#!/usr/bin/env python3
from pathlib import Path
def req(text,markers,owner):
    missing=[m for m in markers if m not in text]
    if missing: raise SystemExit(f"{owner}: missing build-once marker(s): "+", ".join(missing))
def main():
    pr=Path(".github/workflows/reader-pr-ci.yml").read_text(); producer=Path(".github/workflows/chaptera-reader-windows-binary.yml").read_text(); reader=Path(".github/workflows/chaptera-reader-windows.yml").read_text(); smoke=Path(".github/workflows/chaptera-reader-windows-smoke.yml").read_text(); visual=Path(".github/workflows/carlton-reader-visual-oracle.yml").read_text()
    req(pr,("reader-windows-binary:","artifact_name: chaptera-reader-windows-binary-${{ github.sha }}","reader_binary_artifact_name: chaptera-reader-windows-binary-${{ github.sha }}","needs: [classify, tier-a, reader-windows-binary]"),"reader-pr-ci")
    req(producer,('candidate_sha = "${{ github.sha }}"','schema_version = "chaptera.reader-windows-binary.v1"',"cargo build -p chaptera-desktop --release --features reader-only","validate_reader_binary_artifact.py","retention-days: 1"),"producer")
    for owner,text in (("reader",reader),("smoke",smoke),("visual",visual)): req(text,("reader_binary_artifact_name:","actions/download-artifact@v4","validate_reader_binary_artifact.py"),owner)
    if "if: inputs.reader_binary_artifact_name == ''" not in reader: raise SystemExit("reader fallback not conditional")
    if "if: inputs.reader_binary_artifact_name == ''" not in smoke: raise SystemExit("smoke fallback not conditional")
    if "inputs.run_windows_shared_core_smoke && inputs.reader_binary_artifact_name == ''" not in visual: raise SystemExit("visual fallback not conditional")
    def job(name, next_name):
        return pr.split("\n  " + name + ":\n", 1)[1].split(
            "\n  " + next_name + ":\n", 1
        )[0]
    prebuild = job("reader-windows-binary", "reader-windows-smoke")
    req(prebuild, (
        "needs: classify",
        "needs.classify.outputs.reader_windows == 'true'",
        "needs.classify.outputs.reader_windows_smoke == 'true'",
        "artifact_name: chaptera-reader-windows-binary-",
    ), "parallel Reader prebuild")
    if "needs.tier-a.result" in prebuild or "needs: [classify, tier-a]" in prebuild:
        raise SystemExit("Reader prebuild still serial after Tier A")
    for name, next_name in (
        ("reader-windows-smoke", "reader-windows"),
        ("reader-windows", "editor-windows"),
        ("visual-oracle", "cloud-reference"),
    ):
        part = job(name, next_name)
        req(part, (
            "needs: [classify, tier-a, reader-windows-binary]",
            "(needs.tier-a.result == 'success' || needs.tier-a.result == 'skipped')",
            "needs.reader-windows-binary.result == 'success'",
            "reader_binary_artifact_name: chaptera-reader-windows-binary-",
        ), name + " Tier A and exact binary product gate")
    print("Reader build-once wiring guard: ok"); return 0
if __name__=="__main__": raise SystemExit(main())
