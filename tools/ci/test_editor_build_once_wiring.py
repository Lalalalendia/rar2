#!/usr/bin/env python3
from pathlib import Path
MIGRATED=(".github/workflows/editor-desktop-continuity-v2-windows.yml",".github/workflows/editor-fixed-pdf-current-revision.yml",".github/workflows/carlton-march-editor-sample-smoke.yml")
def req(text,markers,owner):
    missing=[m for m in markers if m not in text]
    if missing: raise SystemExit(f"{owner}: missing Editor build-once marker(s): "+", ".join(missing))
def main():
    umbrella=Path(".github/workflows/editor-heavy-pr-ci.yml").read_text(); producer=Path(".github/workflows/chaptera-editor-windows-binary.yml").read_text()
    req(umbrella,("editor-binary:","artifact_name: chaptera-editor-windows-binary-$"+"{{ github.sha }}","editor_binary_artifact_name: chaptera-editor-windows-binary-$"+"{{ github.sha }}","continuity-v2:","fixed-pdf-current-revision:","carlton-editor-smoke:"),"editor-heavy-pr-ci")
    req(producer,('candidate_sha = "' + "$" + '{{ github.sha }}"','schema_version = "chaptera.editor-windows-binary.v1"',"cargo build -p chaptera-desktop --release --bin chaptera-editor","validate_editor_binary_artifact.py","retention-days: 1"),"producer")
    for raw in MIGRATED:
        text=Path(raw).read_text()
        req(text,("workflow_call:","editor_binary_artifact_name:","actions/download-artifact@v4","validate_editor_binary_artifact.py","if: inputs.editor_binary_artifact_name == ''"),raw)
        if "\n  pull_request:\n" in text: raise SystemExit(f"{raw}: direct pull_request trigger duplicates umbrella Editor CI")
    if "Build current fixed-PDF input" not in Path(".github/workflows/editor-fixed-pdf-current-revision.yml").read_text(): raise SystemExit("fixed-PDF consumer lost helper build")
    print("Editor build-once wiring guard: ok"); return 0
if __name__=="__main__": raise SystemExit(main())
