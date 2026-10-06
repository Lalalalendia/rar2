#!/usr/bin/env python3
import hashlib, importlib.util, json, tempfile
from pathlib import Path
MODULE=Path(__file__).with_name("validate_editor_binary_artifact.py")
spec=importlib.util.spec_from_file_location("validate_editor_binary_artifact",MODULE); mod=importlib.util.module_from_spec(spec); assert spec and spec.loader; spec.loader.exec_module(mod)
CANDIDATE="1"*40
def make(root):
    b=root/"chaptera-editor.exe"; b.write_bytes(b"editor-build-once")
    m={"schema_version":mod.SCHEMA,"candidate_sha":CANDIDATE,"pr_head_sha":"2"*40,"target":"x86_64-pc-windows-msvc","toolchain":"1.94.1","rustc_version":"rustc 1.94.1 (test)","profile":"release","features":[],"binary_entry":"chaptera-editor.exe","binary_sha256":hashlib.sha256(b.read_bytes()).hexdigest(),"binary_byte_len":b.stat().st_size}
    (root/"chaptera-editor-binary.json").write_text(json.dumps(m),encoding="utf-8"); return b
def fail(root,candidate,needle):
    try: mod.validate(root,candidate)
    except ValueError as e: assert needle in str(e),(needle,str(e))
    else: raise AssertionError(needle)
def main():
    with tempfile.TemporaryDirectory() as raw:
        root=Path(raw); b=make(root); assert mod.validate(root,CANDIDATE)["profile"]=="release"; fail(root,"3"*40,"candidate mismatch"); b.write_bytes(b.read_bytes()+b"x"); fail(root,CANDIDATE,"SHA-256 mismatch")
    print("Editor binary artifact validator tests: ok")
if __name__=="__main__": main()
