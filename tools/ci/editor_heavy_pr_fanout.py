#!/usr/bin/env python3
import argparse, fnmatch, json, subprocess
from pathlib import Path
CARLTON_PATTERNS=(".github/workflows/carlton-march-editor-sample-smoke.yml","tools/acquire_carlton_march_pair.py","apps/chaptera-desktop/src/acceptance.rs","apps/chaptera-desktop/src/acceptance_cli.rs","vendor/producer-a/crates/pub-editor/**")
def changed_paths(base,head):
    return sorted(dict.fromkeys(p for p in subprocess.check_output(["git","diff","--name-only",f"{base}...{head}"],text=True).splitlines() if p))
def matches(path,pattern):
    return path.startswith(pattern[:-3]) if pattern.endswith("/**") else fnmatch.fnmatchcase(path,pattern)
def classify_carlton(paths):
    return any(matches(path,pattern) for path in paths for pattern in CARLTON_PATTERNS)
def main():
    p=argparse.ArgumentParser(); p.add_argument("--base",required=True); p.add_argument("--head",required=True); p.add_argument("--github-output",type=Path,required=True); p.add_argument("--receipt",type=Path,required=True); a=p.parse_args()
    paths=changed_paths(a.base,a.head); carlton=classify_carlton(paths)
    a.receipt.parent.mkdir(parents=True,exist_ok=True); a.receipt.write_text(json.dumps({"schema":"chaptera.editor-heavy-pr-fanout.v1","base":a.base,"head":a.head,"changed_paths":paths,"carlton_editor_smoke":carlton},indent=2,sort_keys=True)+"\n",encoding="utf-8")
    with a.github_output.open("a",encoding="utf-8") as h: h.write(f"carlton_editor_smoke={'true' if carlton else 'false'}\n")
    return 0
if __name__=="__main__": raise SystemExit(main())
