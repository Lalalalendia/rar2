"""Offline tests: never call Notion or use credentials."""
import sys
import unittest
from pathlib import Path
sys.path.insert(0,str(Path(__file__).parent))
import canonical_v5_identity_receipt as m
PID="3f132a84-beec-81b2-8e76-ceed28680f77"
def page():
    def rt(v):return {"type":"rich_text","rich_text":[{"plain_text":v}]}
    return {"object":"page","id":PID,"last_edited_time":"2026-10-18T00:00:00Z","parent":{"type":"data_source_id","data_source_id":m.CANONICAL_CLAIMS_SOURCE_ID},"properties":{"Claim ID":{"type":"unique_id","unique_id":{"prefix":"PUB-C-","number":154}},"Semantic Identity v1":{"id":"Q2FITg","type":"formula"},"Semantic Revision":{"type":"number","number":1},"Status":{"type":"select","select":{"name":"OPEN"}},"Current Wording":rt("a"),"Scope / Limits":rt("b"),"Do Not Infer":rt("c")}}
def prop(x="original"):
    return {"object":"property_item","id":"Q2FITg","type":"formula","formula":{"type":"string","string":x}}
def check(seq):
    iterator=iter(seq)
    return m.observe(lambda path:next(iterator),page_id=PID,expected_claim_id="PUB-C-154",revision=1,status="OPEN")
class Cases(unittest.TestCase):
    def test_read_only_exact(self):
        r=check([page(),prop(),page(),prop()])
        self.assertEqual(r["observed_semantic_identity_v1"],"original")
        self.assertFalse(r["v5_admission_ready"])
    def test_same_revision_body_drift(self):
        changed=page();changed["properties"]["Current Wording"]["rich_text"][0]["plain_text"]="changed"
        with self.assertRaises(m.ObservationError):check([page(),prop(),changed,prop()])
    def test_formula_drift(self):
        with self.assertRaises(m.ObservationError):check([page(),prop(),page(),prop("changed")])
    def test_unresolved_formula(self):
        with self.assertRaises(m.ObservationError):check([page(),prop("formulaResult://opaque"),page(),prop()])
    def test_missing_scope(self):
        changed=page();changed["properties"]["Scope / Limits"]["rich_text"]=[]
        with self.assertRaises(m.ObservationError):check([changed,prop(),changed,prop()])
    def test_wrong_source(self):
        changed=page();changed["parent"]["data_source_id"]="00000000-0000-0000-0000-000000000000"
        with self.assertRaises(m.ObservationError):check([changed,prop(),changed,prop()])
    def test_revision_gate(self):
        with self.assertRaises(m.ObservationError):m.observe(lambda path:page(),page_id=PID,expected_claim_id="PUB-C-154",revision=2,status="OPEN")
    def test_truncation(self):
        changed=page();changed["properties"]["Current Wording"]["has_more"]=True
        with self.assertRaises(m.ObservationError):check([changed,prop(),changed,prop()])
    def test_redirect_block(self):
        from urllib.request import Request
        req=Request("https://api.notion.com/v1/pages/x",headers={"Authorization":"Bearer token"})
        self.assertIsNone(m._RejectRedirect().redirect_request(req,None,302,"Found",{},"https://other.example"))
    def test_no_private_output_in_git(self):
        import tempfile
        with tempfile.TemporaryDirectory() as td:
            with self.assertRaises(m.ObservationError):
                m.save_private_receipt({"x":1},Path(td)/"repo"/"r.json",repo_root=Path(td)/"repo")
    def test_no_overwrite(self):
        import tempfile
        with tempfile.TemporaryDirectory() as td:
            out=Path(td)/"r.json";out.write_text("original")
            with self.assertRaises(m.ObservationError):
                m.save_private_receipt({"x":1},out,repo_root=Path(td)/"checkout")
            self.assertEqual(out.read_text(),"original")
    def test_bad_property_type(self):
        bad=prop();bad["formula"]={"type":"number","number":1}
        with self.assertRaises(m.ObservationError):check([page(),bad,page(),bad])
if __name__=="__main__":unittest.main()
