#!/usr/bin/env python3
from pathlib import Path
CONSUMERS=(
 ".github/workflows/editor-desktop-continuity-v2-windows.yml",
 ".github/workflows/editor-fixed-pdf-current-revision.yml",
 ".github/workflows/carlton-march-editor-sample-smoke.yml",
)
def require(text,markers,owner):
    missing=[m for m in markers if m not in text]
    if missing: raise SystemExit(f"{owner}: missing Editor build-once substrate marker(s): "+", ".join(missing))
def main():
    producer=Path(".github/workflows/chaptera-editor-windows-binary.yml").read_text()
    require(producer,(
      'candidate_sha = "${{ github.sha }}"',
      'schema_version = "chaptera.editor-windows-binary.v1"',
      "cargo build -p chaptera-desktop --release --bin chaptera-editor",
      "validate_editor_binary_artifact.py","retention-days: 1"),"producer")
    for raw in CONSUMERS:
        text=Path(raw).read_text()
        require(text,("workflow_call:","pull_request:","editor_binary_artifact_name:","actions/download-artifact@v4","validate_editor_binary_artifact.py","if: inputs.editor_binary_artifact_name == ''"),raw)
    if "Build current fixed-PDF input" not in Path(".github/workflows/editor-fixed-pdf-current-revision.yml").read_text():
        raise SystemExit("fixed-PDF consumer lost non-Editor helper build")
    print("Editor build-once substrate wiring guard: ok"); return 0
if __name__=="__main__": raise SystemExit(main())
