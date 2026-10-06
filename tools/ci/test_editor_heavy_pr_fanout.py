#!/usr/bin/env python3
import importlib.util
from pathlib import Path
MODULE=Path(__file__).with_name("editor_heavy_pr_fanout.py"); spec=importlib.util.spec_from_file_location("editor_heavy_pr_fanout",MODULE); mod=importlib.util.module_from_spec(spec); assert spec and spec.loader; spec.loader.exec_module(mod)
def main():
    for p in (".github/workflows/carlton-march-editor-sample-smoke.yml","tools/acquire_carlton_march_pair.py","apps/chaptera-desktop/src/acceptance.rs","apps/chaptera-desktop/src/acceptance_cli.rs","vendor/producer-a/crates/pub-editor/src/imported_paragraph_alignment_v1.rs"): assert mod.classify_carlton([p]) is True,p
    for p in ("README.md","crates/chaptera-scene-instance/src/lib.rs","tools/run_editor_fixed_pdf_current_revision_v2.py"): assert mod.classify_carlton([p]) is False,p
    print("Editor heavy PR fanout tests: ok")
if __name__=="__main__": main()
