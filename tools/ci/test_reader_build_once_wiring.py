#!/usr/bin/env python3
from pathlib import Path
def req(text,markers,owner):
    missing=[m for m in markers if m not in text]
    if missing: raise SystemExit(f"{owner}: missing build-once marker(s): "+", ".join(missing))
def assert_safe_visual_split(pr, visual):
    no_smoke = pr.split("\n  visual-oracle:\n", 1)[1].split(
        "\n  visual-oracle-with-smoke:\n", 1)[0]
    smoke = pr.split("\n  visual-oracle-with-smoke:\n", 1)[1].split(
        "\n  cloud-reference:\n", 1)[0]
    common = (
        "uses: ./.github/workflows/carlton-reader-visual-oracle.yml",
        "run_typography_golden: ${{ needs.classify.outputs.typography_golden == 'true' }}",
        "run_windows_shared_core_smoke: ${{ needs.classify.outputs.reader_windows_smoke == 'true' }}",
        "reader_binary_artifact_name: chaptera-reader-windows-binary-",
        "!cancelled() && always()",
    )
    no_gate = (
        "needs: [classify, tier-a]",
        "&& needs.classify.outputs.reader_windows_smoke != 'true' &&",
        "(needs.tier-a.result == 'success' || needs.tier-a.result == 'skipped')",
    )
    yes_gate = (
        "needs: [classify, tier-a, reader-windows-binary]",
        "&& needs.classify.outputs.reader_windows_smoke == 'true' &&",
        "(needs.tier-a.result == 'success' || needs.tier-a.result == 'skipped')",
        "needs.reader-windows-binary.result == 'success'",
    )
    req(no_smoke, common + no_gate, "no-smoke visual oracle")
    req(smoke, common + yes_gate, "shared-core visual oracle")
    if "needs.reader-windows-binary.result" in no_smoke:
        raise SystemExit("no-smoke visual oracle waits for unused Reader binary")
    if no_smoke.split("    with:\n", 1)[1].rstrip() != smoke.split("    with:\n", 1)[1].rstrip():
        raise SystemExit("visual-oracle caller inputs diverged")
    req(visual, (
        "if: inputs.run_windows_shared_core_smoke && inputs.reader_binary_artifact_name != ''",
        "if: inputs.run_windows_shared_core_smoke && inputs.reader_binary_artifact_name == ''",
    ), "underlying shared-core binary use")
    mutations = [
        (no_smoke.replace(no_gate[0], "needs: classify", 1), common + no_gate),
        (no_smoke.replace(no_gate[1], "&& needs.classify.outputs.reader_windows_smoke == 'true' &&", 1), common + no_gate),
        (smoke.replace(yes_gate[0], "needs: [classify, tier-a]", 1), common + yes_gate),
        (smoke.replace(yes_gate[3], "true", 1), common + yes_gate),
    ]
    for candidate, markers in mutations:
        if candidate == no_smoke or candidate == smoke:
            raise SystemExit("unapplied visual oracle negative control")
        try:
            req(candidate, markers, "visual oracle negative control")
        except SystemExit:
            continue
        raise SystemExit("unsafe visual oracle graph escaped the negative guard")


def main():
    pr=Path(".github/workflows/reader-pr-ci.yml").read_text(); producer=Path(".github/workflows/chaptera-reader-windows-binary.yml").read_text(); reader=Path(".github/workflows/chaptera-reader-windows.yml").read_text(); smoke=Path(".github/workflows/chaptera-reader-windows-smoke.yml").read_text(); visual=Path(".github/workflows/carlton-reader-visual-oracle.yml").read_text()
    req(pr,("reader-windows-binary:","artifact_name: chaptera-reader-windows-binary-${{ github.sha }}","reader_binary_artifact_name: chaptera-reader-windows-binary-${{ github.sha }}","needs: [classify, tier-a, reader-windows-binary]"),"reader-pr-ci")
    req(producer,('candidate_sha = "${{ github.sha }}"','schema_version = "chaptera.reader-windows-binary.v1"',"cargo build -p chaptera-desktop --release --features reader-only","validate_reader_binary_artifact.py","retention-days: 1"),"producer")
    for owner,text in (("reader",reader),("smoke",smoke),("visual",visual)): req(text,("reader_binary_artifact_name:","actions/download-artifact@v4","validate_reader_binary_artifact.py"),owner)
    if "if: inputs.reader_binary_artifact_name == ''" not in reader: raise SystemExit("reader fallback not conditional")
    if "if: inputs.reader_binary_artifact_name == ''" not in smoke: raise SystemExit("smoke fallback not conditional")
    if "inputs.run_windows_shared_core_smoke && inputs.reader_binary_artifact_name == ''" not in visual: raise SystemExit("visual fallback not conditional")
    assert_safe_visual_split(pr, visual)
    print("Reader build-once wiring guard: ok"); return 0
if __name__=="__main__": raise SystemExit(main())
