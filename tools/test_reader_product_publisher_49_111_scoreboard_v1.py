#!/usr/bin/env python3
"""Synthetic contract validation only; NEVER counted as physical Product proof."""
import copy
import hashlib
import unittest
from pathlib import Path
from reader_publisher_lo_49_111_scoreboard_v1 import load_frozen,read_publisher_registry
from reader_product_publisher_49_111_scoreboard_v1 import METHOD,PROOFS,ROUTE,SCHEMA,score_product
ROOT=Path(__file__).resolve().parent.parent
FROZEN=ROOT/"tools/corpus/receipts/publisher-lo-t999-49-111-frozen-v1.json"
REF=ROOT/"tools/corpus/receipts/publisher-visual-golden-batch-01-fingerprint-v1.json"
PAIRS=ROOT/"tools/corpus/receipts/publisher-visual-golden-batch-01-pairs.csv"
HEAD="e"*40
def synthetic(frozen,refs):
    items=[]
    for fixture,sha,pdf,n,_a,_b,lo in frozen["rows"]:
        pages=[{"page":ordinal,
                "source_page_id_sha256":hashlib.sha256(f"{fixture}/{ordinal}/id".encode()).hexdigest(),
                "chromium_screenshot_sha256":hashlib.sha256(f"{fixture}/{ordinal}/png".encode()).hexdigest(),
                "reference_grid_sha256":refs[fixture]["pages"][ordinal-1]["grid_sha256"],
                "reference_surface_stage":"unknown",
                "reference_media_extent_delta_pt":{"width":0.0,"height":0.0},
                "changed_cell_count":old} for ordinal,_lo,old in lo]
        items.append({"fixture":fixture,"source_sha256":sha,"publisher_pdf_sha256":pdf,
                      "repository_commit_sha":HEAD,"route":ROUTE,"status":"rendered_partial",
                      "scene_fidelity":"partial","physical_proofs":{k:True for k in PROOFS},
                      "external_fetch_count":0,"page_error_count":0,
                      "candidate_page_count":n,"pages":pages})
    return {"schema":SCHEMA,"repository_commit_sha":HEAD,"route":ROUTE,
            "method":METHOD,"source_free_artifact":True,
            "publisher_pixel_exact_parity":False,
            "reference_surface_stage":"unknown","fixtures":items}
class GateTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.frozen=load_frozen(FROZEN)
        cls.refs,cls.pairs=read_publisher_registry(REF,PAIRS)
    def setUp(self):
        self.v=synthetic(self.frozen,self.refs)
    def score(self):
        return score_product(self.v,self.frozen,self.refs,self.pairs,HEAD)
    def test_historic_frozen_numeric_reproduction_is_not_product_claim(self):
        r=self.score()
        self.assertEqual([r["page_scores"][k] for k in ("libreoffice_wins","reader_wins","near_ties")],[85,17,9])
        self.assertEqual([r["document_scores"][k] for k in ("libreoffice_wins","reader_wins","near_ties")],[26,8,5])
        self.assertEqual((r["ranked_pages"],r["ranked_documents"],len(r["predeclared_ineligible"])),(111,39,10))
        self.assertFalse(r["native_publisher_pixel_exact_parity"])
    def test_negative_controls(self):
        cases=[
            lambda v:v["fixtures"].pop(),
            lambda v:v.__setitem__("route","isolated_worker_direct"),
            lambda v:v["fixtures"][0].__setitem__("route","isolated_worker_direct"),
            lambda v:v["fixtures"][0]["physical_proofs"].__setitem__("oidc_pkce",False),
            lambda v:v["fixtures"][0]["physical_proofs"].__setitem__("independent_host_image_admission",False),
            lambda v:v["fixtures"][0].__setitem__("status","rejected"),
            lambda v:v["fixtures"][0].__setitem__("source_sha256","0"*64),
            lambda v:v["fixtures"][0].__setitem__("publisher_pdf_sha256","0"*64),
            lambda v:v["fixtures"][0].__setitem__("candidate_page_count",2),
            lambda v:v["fixtures"][0].__setitem__("pages",[]),
            lambda v:v["fixtures"][0]["pages"][0].__setitem__("changed_cell_count",True),
            lambda v:v["fixtures"][0]["pages"][0].__setitem__("changed_cell_count",4097),
            lambda v:v["fixtures"][0]["pages"][0].__setitem__("reference_grid_sha256","0"*64),
            lambda v:v["fixtures"][0]["pages"][0].__setitem__("chromium_screenshot_sha256","bad"),
            lambda v:v["fixtures"][0]["pages"][0]["reference_media_extent_delta_pt"].__setitem__("width",1.0),
            lambda v:v.__setitem__("publisher_pixel_exact_parity",True),
        ]
        for i,modify in enumerate(cases):
            with self.subTest(i=i):
                baseline=self.v
                try:
                    self.v=copy.deepcopy(baseline)
                    modify(self.v)
                    with self.assertRaises((ValueError,KeyError,TypeError)):
                        self.score()
                finally:
                    self.v=baseline
if __name__=="__main__":
    unittest.main()
