import hashlib
import json
from pathlib import Path
import sys
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parent))
import worker

SHA = hashlib.sha256(b"PUB file from private source").hexdigest()

class WorkerTests(unittest.TestCase):
    def fake_request(self, response_overrides=None):
        calls = []
        sid = "guest-test-123"
        root = f"/v1/reader/guest-sessions/{sid}"
        issued = {
            "protocol_version": worker.GUEST_SCHEMA,
            "session_id": sid,
            "access_token": "a" * 64,
            "upload_path": root + "/content",
            "open_path": root + "/open",
        }
        report = {
            "protocol_version": worker.REPORT_SCHEMA,
            "source_sha256": SHA,
            "engine_classification": "supported",
            "state": "opens_normally",
            "content_summary": {"page_count": 1},
            "limitations": [],
            "output_routes": {
                "read_only_preview": "available", "salvage_recovery": "not_applicable",
                "editable_idml": "not_verified", "editable_odg": "not_verified",
            },
            "recommended_next_step": "migration_pilot_preview",
        }
        opened = {
            "protocol_version": worker.GUEST_SCHEMA, "session_id": sid,
            "source_sha256": SHA, "classification": "supported",
            "compatibility_report": report,
            "scene": {"stories": [{"text": "PRIVATE STORY"}]},
        }
        payloads = [
            issued,
            {"protocol_version": worker.GUEST_SCHEMA, "session_id": sid, "state": "stored"},
            opened,
        ]
        if response_overrides:
            response_overrides(payloads)
        def handler(origin, path, method, body, headers, max_response=worker.MAX_REPORT_BYTES):
            calls.append((origin,path,method,body,headers))
            return payloads[len(calls)-1]
        return handler, calls

    def test_exact_source_and_no_story_data_leave_worker(self):
        handler, calls = self.fake_request()
        with patch.object(worker,"json_guest",side_effect=handler):
            report=worker.canonical_report_for(b"PUB file from private source",worker.cloud_origin("https://reader.chaptera.online"))
        self.assertEqual(report["source_sha256"],SHA)
        self.assertNotIn("scene",report)
        self.assertNotIn("PRIVATE STORY",json.dumps(report))
        self.assertEqual(len(calls),3)
        self.assertEqual(calls[1][3],b"PUB file from private source")
        self.assertEqual(calls[2][2],"POST")

    def test_changed_source_sha_fails_closed(self):
        def mutate(payloads): payloads[2]["compatibility_report"]["source_sha256"]="b"*64
        handler,_=self.fake_request(mutate)
        with patch.object(worker,"json_guest",side_effect=handler):
            with self.assertRaisesRegex(worker.BridgeError,"canonical_report_protocol_mismatch"):
                worker.canonical_report_for(b"PUB file from private source","https://reader.chaptera.online")

    def test_untrusted_response_path_not_followed(self):
        def mutate(payloads): payloads[0]["open_path"]="https://attacker.example/steal"
        handler,calls=self.fake_request(mutate)
        with patch.object(worker,"json_guest",side_effect=handler):
            with self.assertRaisesRegex(worker.BridgeError,"guest_issue_path_invalid"):
                worker.canonical_report_for(b"PUB file from private source","https://reader.chaptera.online")
        self.assertEqual(len(calls),1)

    def test_rejected_or_unsupported_without_report_is_not_fake_unsupported(self):
        def mutate(payloads):
            payloads[2]["classification"]="rejected"
            payloads[2]["compatibility_report"]=None
            payloads[2]["source_sha256"]=None
        handler,_=self.fake_request(mutate)
        with patch.object(worker,"json_guest",side_effect=handler):
            with self.assertRaises(worker.BridgeError):
                worker.canonical_report_for(b"PUB file from private source","https://reader.chaptera.online")
        self.assertEqual(worker.failure_receipt()["compatibility"],"failed")

    def test_private_origin_enforced(self):
        for bad in ["http://reader.chaptera.online","https://evil.example","https://reader.chaptera.online/redirect",
                    "https://user:secret@reader.chaptera.online","https://reader.chaptera.online.evil.test"]:
            with self.assertRaises(worker.BridgeError):
                worker.cloud_origin(bad)
        self.assertEqual(worker.cloud_origin("https://reader.chaptera.online/"),"https://reader.chaptera.online")

    def test_source_size_guard(self):
        handler,calls=self.fake_request()
        with patch.object(worker,"json_guest",side_effect=handler):
            with self.assertRaisesRegex(worker.BridgeError,"source_size_invalid"):
                worker.canonical_report_for(b"","https://reader.chaptera.online")
        self.assertEqual(len(calls),0)

if __name__=="__main__":
    unittest.main()
