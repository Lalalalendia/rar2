#!/usr/bin/env python3
import hashlib,sys,tempfile,unittest
from pathlib import Path
from unittest.mock import patch
sys.path.insert(0,str(Path(__file__).resolve().parent))
import pub_container_extract as c

class T(unittest.TestCase):
    def test_path_safety(self):
        self.assertTrue(c.safe_member("a/b.pub"))
        self.assertFalse(c.safe_member("../x.pub"))
        self.assertFalse(c.safe_member("/abs.pub"))
    def test_kind(self):
        self.assertEqual(c.ext_kind("x.PUB"),"pub")
        self.assertEqual(c.ext_kind("x.iso"),"container")
        self.assertEqual(c.ext_kind("x.txt"),"")

    def test_verified_root_uses_fallback_url(self):
        payload=b"verified-root"
        sha=hashlib.sha256(payload).hexdigest()
        row={
            "direct_url":"https://archive.example/root.iso",
            "fallback_url":"https://cdn.example/root.iso",
            "expected_root_sha256":sha,
            "expected_root_size_bytes":str(len(payload)),
        }
        meta={
            "sha256":sha,
            "size":len(payload),
            "final_url":"https://cdn.example/root.iso",
            "content_type":"application/octet-stream",
        }
        with tempfile.TemporaryDirectory() as td:
            target=Path(td)/"root.iso"
            with patch.object(c,"fetch",side_effect=[OSError("primary down"),meta]) as mocked:
                used,got,errors=c._fetch_verified_root(row,target,1,1024)
        self.assertEqual(mocked.call_count,2)
        self.assertEqual(used,row["fallback_url"])
        self.assertEqual(got["sha256"],sha)
        self.assertEqual(len(errors),1)

    def test_fetch_retries_transient_failure(self):
        payload=b"publisher-container"
        class Response:
            headers={"Content-Type":"application/octet-stream"}
            def __enter__(self): return self
            def __exit__(self,*_): return False
            def read(self,_size=-1):
                if getattr(self,"done",False): return b""
                self.done=True
                return payload
            def geturl(self): return "https://example.test/root.iso"

        with tempfile.TemporaryDirectory() as td:
            target=Path(td)/"root.iso"
            with patch.object(c,"urlopen",side_effect=[OSError("transient"),Response()]) as mocked:
                meta=c.fetch(
                    "https://example.test/root.iso",
                    target,
                    timeout=1,
                    max_bytes=1024,
                    retries=1,
                    retry_delay=0,
                )
            self.assertEqual(mocked.call_count,2)
            self.assertEqual(target.read_bytes(),payload)
            self.assertEqual(meta["sha256"],hashlib.sha256(payload).hexdigest())
            self.assertEqual(meta["fetch_attempt"],2)

if __name__=="__main__": unittest.main()
