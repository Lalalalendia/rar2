#!/usr/bin/env python3
from __future__ import annotations
import argparse, hashlib, json, re, sys
from pathlib import Path
SCHEMA="chaptera.editor-windows-binary.v1"
SHA256_RE=re.compile(r"^[0-9a-f]{64}$")
GIT_SHA_RE=re.compile(r"^[0-9a-f]{40}$")
def sha256_file(path: Path) -> str:
    h=hashlib.sha256()
    with path.open("rb") as f:
        for chunk in iter(lambda:f.read(1024*1024),b""): h.update(chunk)
    return h.hexdigest()
def validate(artifact_dir: Path, expected_candidate_sha: str) -> dict:
    artifact_dir=artifact_dir.resolve()
    mp=artifact_dir/"chaptera-editor-binary.json"; bp=artifact_dir/"chaptera-editor.exe"
    if not mp.is_file(): raise ValueError(f"missing Editor binary manifest: {mp}")
    if not bp.is_file(): raise ValueError(f"missing Editor binary: {bp}")
    m=json.loads(mp.read_text(encoding="utf-8-sig"))
    if m.get("schema_version")!=SCHEMA: raise ValueError("Editor binary artifact schema mismatch")
    if not GIT_SHA_RE.fullmatch(expected_candidate_sha): raise ValueError("expected candidate SHA must be a full lowercase git SHA")
    if m.get("candidate_sha")!=expected_candidate_sha: raise ValueError(f"Editor binary candidate mismatch: {m.get('candidate_sha')!r} != {expected_candidate_sha!r}")
    if m.get("target")!="x86_64-pc-windows-msvc": raise ValueError("Editor binary target mismatch")
    if m.get("toolchain")!="1.94.1": raise ValueError("Editor binary toolchain mismatch")
    if not str(m.get("rustc_version","")).startswith("rustc 1.94.1 "): raise ValueError("Editor binary rustc identity mismatch")
    if m.get("profile")!="release": raise ValueError("Editor binary profile mismatch")
    if m.get("features")!=[]: raise ValueError("Editor binary feature set mismatch")
    if m.get("binary_entry")!="chaptera-editor.exe": raise ValueError("Editor binary entry mismatch")
    actual_hash=sha256_file(bp); expected_hash=m.get("binary_sha256")
    if not isinstance(expected_hash,str) or not SHA256_RE.fullmatch(expected_hash): raise ValueError("Editor binary manifest SHA-256 is malformed")
    if actual_hash!=expected_hash: raise ValueError(f"Editor binary SHA-256 mismatch: {actual_hash} != {expected_hash}")
    actual_bytes=bp.stat().st_size
    if m.get("binary_byte_len")!=actual_bytes: raise ValueError(f"Editor binary byte length mismatch: {actual_bytes} != {m.get('binary_byte_len')!r}")
    return m
def main()->int:
    p=argparse.ArgumentParser(); p.add_argument("--artifact-dir",type=Path,required=True); p.add_argument("--expected-candidate-sha",required=True); a=p.parse_args()
    try: m=validate(a.artifact_dir,a.expected_candidate_sha)
    except (OSError,ValueError,json.JSONDecodeError) as e: print(f"Editor binary artifact validation failed: {e}",file=sys.stderr); return 2
    print(f"Editor binary artifact validated: candidate={m['candidate_sha']} sha256={m['binary_sha256']}"); return 0
if __name__=="__main__": raise SystemExit(main())
