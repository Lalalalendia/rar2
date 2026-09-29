#!/usr/bin/env python3
"""Build a hash-addressed strict Publisher corpus from trusted local inputs.

This is intentionally offline. Network recovery belongs to the existing
acquisition/container tools. This tool validates, deduplicates, materializes,
and emits a receipt against the current durable SHA authority.
"""
from __future__ import annotations

import argparse,hashlib,json,re,shutil,sys
from collections import defaultdict
from pathlib import Path

sys.path.insert(0,str(Path(__file__).resolve().parent))
import harvest_pub

SHA_RE=re.compile(r"^[0-9a-f]{64}$")


def file_sha256(path:Path)->str:
    h=hashlib.sha256()
    with path.open("rb") as f:
        for chunk in iter(lambda:f.read(1024*1024),b""):
            h.update(chunk)
    return h.hexdigest()


def load_authority(path:Path)->set[str]:
    out=set()
    for token in path.read_text(encoding="utf-8").split():
        token=token.casefold()
        if SHA_RE.fullmatch(token): out.add(token)
    if not out:
        raise ValueError(f"no SHA-256 values found in authority {path}")
    return out


def scan(root:Path,label:str,observations:dict[str,list[dict]],rejections:list[dict])->None:
    if not root.exists():
        raise FileNotFoundError(root)
    for path in sorted(root.rglob("*.pub")):
        try:
            data=path.read_bytes()
            sha=hashlib.sha256(data).hexdigest()
            stem=path.stem.casefold()
            if SHA_RE.fullmatch(stem) and stem!=sha:
                raise ValueError(f"hash-named file drift: name={stem} bytes={sha}")
            cls,hints=harvest_pub.classify(data)
            if cls!="cfb_publisher_hint":
                raise ValueError(f"classification={cls}")
            observations[sha].append({
                "source":label,
                "path":str(path),
                "size_bytes":len(data),
                "publisher_hints":hints,
            })
        except Exception as exc:
            rejections.append({"source":label,"path":str(path),"error":f"{type(exc).__name__}: {exc}"})


def main()->int:
    ap=argparse.ArgumentParser()
    ap.add_argument("--input",action="append",required=True,metavar="LABEL=DIR")
    ap.add_argument("--authority",type=Path,required=True)
    ap.add_argument("--out",type=Path,required=True)
    ap.add_argument("--min-count",type=int,default=1000)
    args=ap.parse_args()

    authority=load_authority(args.authority)
    observations:dict[str,list[dict]]=defaultdict(list)
    rejections:list[dict]=[]
    inputs=[]
    for spec in args.input:
        if "=" not in spec:
            raise SystemExit(f"invalid --input {spec!r}; expected LABEL=DIR")
        label,raw=spec.split("=",1)
        root=Path(raw)
        inputs.append({"label":label,"root":str(root)})
        scan(root,label,observations,rejections)

    corpus_dir=args.out/"native"
    corpus_dir.mkdir(parents=True,exist_ok=True)
    rows=[]
    for sha in sorted(observations):
        source_path=Path(observations[sha][0]["path"])
        target=corpus_dir/f"{sha}.pub"
        if target.exists():
            if file_sha256(target)!=sha:
                raise ValueError(f"existing output collision at {target}")
        else:
            shutil.copyfile(source_path,target)
        rows.append({
            "sha256":sha,
            "size_bytes":source_path.stat().st_size,
            "authority_member":sha in authority,
            "sources":sorted({x["source"] for x in observations[sha]}),
            "source_paths":[x["path"] for x in observations[sha]],
        })

    sha_list="".join(r["sha256"]+"\n" for r in rows)
    args.out.mkdir(parents=True,exist_ok=True)
    (args.out/"sha256.txt").write_text(sha_list,encoding="utf-8")
    (args.out/"manifest.json").write_text(json.dumps(rows,indent=2,ensure_ascii=False),encoding="utf-8")
    (args.out/"rejections.json").write_text(json.dumps(rejections,indent=2,ensure_ascii=False),encoding="utf-8")
    authority_overlap=sum(bool(r["authority_member"]) for r in rows)
    summary={
        "schema":"rar2-materialized-pub-corpus-v1",
        "inputs":inputs,
        "min_count":args.min_count,
        "unique_publisher_cfb":len(rows),
        "authority_count":len(authority),
        "authority_overlap":authority_overlap,
        "outside_authority":len(rows)-authority_overlap,
        "rejections":len(rejections),
        "sha_list_digest":"sha256:"+hashlib.sha256(sha_list.encode("ascii")).hexdigest(),
        "target_satisfied":len(rows)>=args.min_count,
    }
    (args.out/"summary.json").write_text(json.dumps(summary,indent=2),encoding="utf-8")
    print(json.dumps(summary,indent=2))
    if len(rows)<args.min_count:
        raise SystemExit(f"materialized corpus below target: {len(rows)} < {args.min_count}")
    return 0

if __name__=="__main__": raise SystemExit(main())
