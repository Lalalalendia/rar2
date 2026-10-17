#!/usr/bin/env python3
import copy
import unittest
from product_full_case_census_v1 import record, combine
from product_full_cohort_shards_v1 import select
from test_product_full_cohort_shards_v1 import sample
HEAD='a'*40

class FailClosedPhysicalCensusTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.plan=select(*sample())
        cls.case=cls.plan['shards'][0]['fixtures'][0]

    def rows(self):
        return [record(c,HEAD,1,31,None,None)
                for c in self.plan['shards'][0]['fixtures']]

    def test_real_rejected_is_not_score(self):
        result=combine(self.plan,0,HEAD,self.rows())
        self.assertEqual(result['rejected_cases'],2)
        self.assertFalse(result['full_shard_admitted'])
        self.assertFalse(result['global_product_score_published'])

    def test_sanitized_known_http_failure(self):
        c=self.case
        o={'schema':'chaptera.product-oidc-worst-census-failure.v1',
           'fixture':c['id'],'repository_commit_sha':HEAD,
           'failure_code':'product_http_denial',
           'public_error_code':'product_replay_worker_failed',
           'current_http_status':200,'scene_http_status':500}
        r=record(c,HEAD,1,30,o,None)
        self.assertEqual(r['scene_http_status'],500)
        self.assertEqual(r['public_error_code'],'product_replay_worker_failed')
        self.assertNotIn('source_sha256',r)

    def test_private_input_is_redacted(self):
        c=self.case
        o={'schema':'chaptera.product-oidc-worst-census-failure.v1',
           'fixture':c['id'],'repository_commit_sha':HEAD,
           'failure_code':'SECRET1234','public_error_code':'/private/source.pub'}
        r=record(c,HEAD,1,29,o,None)
        self.assertEqual(r['failure_code'],'unclassified')
        self.assertEqual(r['public_error_code'],'redacted')
        self.assertNotIn('SECRET',str(r))

    def test_missing_success_proof_is_rejected(self):
        with self.assertRaisesRegex(ValueError,'admitted'):
            record(self.case,HEAD,0,12,None,None)

    def test_failed_source_cannot_be_admitted(self):
        with self.assertRaisesRegex(ValueError,'cannot be accepted'):
            record(self.case,HEAD,1,31,None,{})

    def test_missing_case_forbidden(self):
        with self.assertRaisesRegex(ValueError,'incomplete'):
            combine(self.plan,0,HEAD,self.rows()[:-1])

    def test_duplicate_case_forbidden(self):
        rows=self.rows()
        with self.assertRaisesRegex(ValueError,'duplicate'):
            combine(self.plan,0,HEAD,[rows[0],copy.deepcopy(rows[0])])

    def test_wrong_head_forbidden(self):
        rows=self.rows();rows[0]['repository_commit_sha']='b'*40
        with self.assertRaisesRegex(ValueError,'stale'):
            combine(self.plan,0,HEAD,rows)

    def test_extra_private_field_forbidden(self):
        rows=self.rows();rows[0]['source_bytes']='private'
        with self.assertRaisesRegex(ValueError,'malformed'):
            combine(self.plan,0,HEAD,rows)

if __name__=='__main__':
    unittest.main()
