#!/usr/bin/env python3
"""Source-neutral positive and adversarial Product 49/111 shard tests."""
from __future__ import annotations

import unittest

from product_full_cohort_shards_v1 import (
    EXCLUDED, EXPECTED_SHARD_PAGES, FROZEN, SHARDS, build, select,
)


def sample():
    frozen = {'rows': []}
    refs, pairs = {}, {}
    eligible = [case for shard in SHARDS for case in shard]
    for number, case in enumerate(eligible + sorted(EXCLUDED), 1):
        source = f'{number:064x}'
        pdf = f'{number + 100:064x}'
        name = f'{case}_{source[:16]}'
        shard = next((shard for shard in SHARDS if case in shard), ())
        n = 1
        if shard and case == shard[0]:
            n += EXPECTED_SHARD_PAGES[SHARDS.index(shard)] - len(shard)
        rows = [[i, 100, 200] for i in range(1, n + 1)] if case in eligible else []
        frozen['rows'].append([name, source, pdf, n, n, n, rows])
        refs[name] = {'pub_sha256': source, 'pdf_sha256': pdf,
                      'pages': [{'page': i} for i in range(1, n + 1)]}
        pairs[name] = {'pub_sha256': source, 'pdf_sha256': pdf,
                       'pdf_pages': str(n), 'pub_bytes': str(10000 + number)}
    return frozen, refs, pairs


class FixedPhysicalCohortTests(unittest.TestCase):
    def setUp(self):
        self.frozen, self.refs, self.pairs = sample()

    def test_exact_nonoverlapping_partition(self):
        plan = select(self.frozen, self.refs, self.pairs)
        self.assertEqual(plan['eligible_documents'], 39)
        self.assertEqual(plan['eligible_pages'], 111)
        self.assertEqual([s['pages'] for s in plan['shards']], [28, 28, 28, 27])
        self.assertEqual([len(s['fixtures']) for s in plan['shards']], [2, 12, 12, 13])
        self.assertFalse(plan['physical_capture_completed'])
        self.assertFalse(plan['product_score_accepted'])

    def test_pdf_sha_forgery_rejected(self):
        key = next(iter(self.pairs))
        self.pairs[key]['pdf_sha256'] = '0' * 64
        with self.assertRaisesRegex(ValueError, 'SHA differs'):
            select(self.frozen, self.refs, self.pairs)

    def test_page_omission_rejected(self):
        row = next(r for r in self.frozen['rows'] if r[0].startswith('075_'))
        row[6] = row[6][:-1]
        with self.assertRaisesRegex(ValueError, 'PAGE ordinals'):
            select(self.frozen, self.refs, self.pairs)

    def test_extra_exclusion_rejected(self):
        row = next(r for r in self.frozen['rows'] if r[0].startswith('020_'))
        row[6] = []
        with self.assertRaisesRegex(ValueError, 'ineligible|denominator'):
            select(self.frozen, self.refs, self.pairs)

    def test_excluded_source_admitted_rejected(self):
        row = next(r for r in self.frozen['rows'] if r[0].startswith('029_'))
        row[6] = [[1, 4, 4]]
        with self.assertRaisesRegex(ValueError, 'ineligible'):
            select(self.frozen, self.refs, self.pairs)

    def test_duplicate_case_prefix_rejected(self):
        a, b = self.frozen['rows'][:2]
        b[0] = a[0][:3] + '_' + b[1][:16]
        with self.assertRaisesRegex(ValueError, 'duplicate'):
            select(self.frozen, self.refs, self.pairs)

    def test_missing_case_rejected(self):
        self.frozen['rows'].pop()
        with self.assertRaisesRegex(ValueError, '49 PUBs'):
            select(self.frozen, self.refs, self.pairs)

    def test_source_size_fence(self):
        key = next(iter(self.pairs))
        self.pairs[key]['pub_bytes'] = '100000000'
        with self.assertRaisesRegex(ValueError, 'size'):
            select(self.frozen, self.refs, self.pairs)

    @unittest.skipUnless(FROZEN.is_file(), 'requires exact rar2 source checkout')
    def test_exact_immutable_real_cohort(self):
        real = build()
        self.assertEqual(real['eligible_documents'], 39)
        self.assertEqual(real['eligible_pages'], 111)
        entries = [f for s in real['shards'] for f in s['fixtures']]
        self.assertEqual(len(entries), 39)
        pub020 = next(f for f in entries if f['id'] == '020')
        self.assertEqual(pub020['source_sha256'],
                         '6c6af8064f91ad5321f76c97c00b1a6564794d92614373334de23e2759294aaa')
        self.assertEqual(pub020['source_bytes'], 9351168)
        self.assertEqual(pub020['pages'], 1)


if __name__ == '__main__':
    unittest.main()
