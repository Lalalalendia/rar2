#!/usr/bin/env python3
from __future__ import annotations

import hashlib,json,subprocess,sys,tempfile
from pathlib import Path
from types import SimpleNamespace

HERE=Path(__file__).resolve().parent
sys.path.insert(0,str(HERE))
import harvest_pub
import pub_container_extract


def fake_pub(tag:bytes)->bytes:
    return harvest_pub.CFB_MAGIC + b"\0"*64 + "Microsoft Publisher".encode("utf-16le") + b"\0"*32 + tag


def main()->int:
    with tempfile.TemporaryDirectory() as td:
        root=Path(td)
        materialized=root/"container"
        data=fake_pub(b"a")
        sha=hashlib.sha256(data).hexdigest()
        args=SimpleNamespace(materialize_dir=materialized)
        path=Path(pub_container_extract._materialize_pub(data,sha,args))
        assert path.name==f"{sha}.pub"
        assert path.read_bytes()==data
        pub_container_extract._verify_expected_root(
            {"expected_root_sha256":sha,"expected_root_size_bytes":str(len(data))},
            {"sha256":sha,"size":len(data)},
        )
        try:
            pub_container_extract._verify_expected_root(
                {"expected_root_sha256":"0"*64},
                {"sha256":sha,"size":len(data)},
            )
        except ValueError:
            pass
        else:
            raise AssertionError("root SHA mismatch must fail")

        donor=root/"donor"; extra=root/"extra"; out=root/"out"
        donor.mkdir(); extra.mkdir()
        d1=fake_pub(b"one"); d2=fake_pub(b"two")
        s1=hashlib.sha256(d1).hexdigest(); s2=hashlib.sha256(d2).hexdigest()
        (donor/f"{s1}.pub").write_bytes(d1)
        (extra/f"{s2}.pub").write_bytes(d2)
        authority=root/"authority.txt"
        authority.write_text(s1+"\n",encoding="utf-8")
        subprocess.run([
            sys.executable,str(HERE/"materialize_corpus_1000.py"),
            "--input",f"donor={donor}",
            "--input",f"extra={extra}",
            "--authority",str(authority),
            "--out",str(out),
            "--min-count","2",
        ],check=True)
        summary=json.loads((out/"summary.json").read_text(encoding="utf-8"))
        assert summary["unique_publisher_cfb"]==2,summary
        assert summary["authority_overlap"]==1,summary
        assert summary["outside_authority"]==1,summary
        assert summary["target_satisfied"] is True,summary
        assert (out/"native"/f"{s1}.pub").exists()
        assert (out/"native"/f"{s2}.pub").exists()
    print("ok")
    return 0


if __name__=="__main__": raise SystemExit(main())
