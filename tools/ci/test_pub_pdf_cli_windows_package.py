"""Fail-closed cheap regression fence for the Windows PUB->PDF preview packet."""
from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[2]
WORKFLOW = ROOT / ".github/workflows/pub-pdf-cli-windows-package.yml"
SCRIPT = ROOT / "tools/ci/package_pub_pdf_cli_windows.ps1"
SOURCE = ROOT / "vendor/producer-a/crates/pub-cli/src/fixed_pdf.rs"


def require(fragment: str, body: str, label: str) -> None:
    if fragment not in body:
        raise AssertionError(f"{label}: required contract missing: {fragment!r}")


def validate(workflow: str, script: str, source: str) -> None:
    for fragment in (
        "  pull_request:",
        "  push:\n    branches: [main]",
        '      - ".github/workflows/pub-pdf-cli-windows-package.yml"',
        '      - "tools/ci/package_pub_pdf_cli_windows.ps1"',
        "  workflow_dispatch:",
        "  contents: read",
        "    if: github.event_name != 'pull_request' && github.ref == 'refs/heads/main'",
        'run: test "$GITHUB_REF" = "refs/heads/main"',
        "python tools/ci/test_pub_pdf_cli_windows_package.py",
        "ParseFile($path, [ref]$tokens, [ref]$errors)",
        "cargo build --manifest-path vendor/producer-a/Cargo.toml -p pub-cli --bin pub --release",
        "package_pub_pdf_cli_windows.ps1 -CandidateSha",
        "retention-days: 7",
        "if-no-files-found: error",
    ):
        require(fragment, workflow, "workflow")
    for forbidden in (
        "contents: write",
        "id-token: write",
        "gh release",
        "softprops/action-gh-release",
        "secrets.",
        "pull_request_target:",
    ):
        if forbidden in workflow:
            raise AssertionError(f"workflow: forbidden release authority: {forbidden!r}")
    for fragment in (
        "Assert-Same \"checked out commit\" $head $CandidateSha",
        'vendor/producer-a/target/release/pub.exe',
        '"UNSIGNED_TECHNICAL_PREVIEW"',
        '"GITHUB_ACTIONS_ARTIFACT_ONLY"',
        '"README.txt", "SHA256SUMS.txt", "manifest.json", "pub.exe"',
        "Expand-Archive -LiteralPath $zip",
        "& $freshExe convert $fixture --to pdf",
        '".loss.json", ".loss.txt"',
        '"SampleNewsletter.pub"',
        '"chaptera-fallback.ttf"',
        '"c0afdb480937e9c1ba70742e35c4eb184969c4f3e326f1e11e172a33bd5fa012"',
        '"a748e8f5dfa0217c3d7420b6eb85196be7ace7bcd5a7d2c152da6fcb685c2a32"',
        '"802b475b0a759b99c0af26498b2a67df46034c5577e0a974ba037b022afcd062"',
        'Assert-Same "cross-OS PDF SHA256"',
        'Assert-Same "cross-OS loss JSON SHA256"',
        'Assert-Same "cross-OS loss text SHA256"',
        'Assert-Same "source report label" $report.source.label "SampleNewsletter.pub"',
        'Assert-Same "fallback report label" $report.typography.fallback_font.label "chaptera-fallback.ttf"',
        "Get-Sha256 $fixture",
        "Get-Sha256 $font",
        'materialize-fallback-font',
        "fixed_flow_receipt.invariants.reshaping_calls",
        "skipped_run_count",
        "packaged_font_or_source = $false",
        "extracted_binary_executed = $true",
    ):
        require(fragment, script, "package script")
    for fragment in (
        'report_path_label(input, "input.pub")',
        'report_path_label(fallback_font, "fallback-font.ttf")',
    ):
        require(fragment, source, "fixed PDF source")
    for forbidden in ("input.display().to_string()", "fallback_font.display().to_string()"):
        if forbidden in source:
            raise AssertionError(f"fixed PDF source: absolute report-path carrier returned: {forbidden!r}")
    for forbidden in ("gh release", "Publish-Module", "Invoke-RestMethod -Method Post"):
        if forbidden in script:
            raise AssertionError(f"package script: forbidden publication action: {forbidden!r}")


class PreviewContract(unittest.TestCase):
    def test_actual_files(self) -> None:
        validate(
            WORKFLOW.read_text(encoding="utf-8"),
            SCRIPT.read_text(encoding="utf-8"),
            SOURCE.read_text(encoding="utf-8"),
        )

    def test_cannot_allow_pr_binary_build(self) -> None:
        workflow = WORKFLOW.read_text(encoding="utf-8")
        script = SCRIPT.read_text(encoding="utf-8")
        source = SOURCE.read_text(encoding="utf-8")
        weakened = workflow.replace(
            "github.event_name != 'pull_request' && github.ref == 'refs/heads/main'",
            "true",
        )
        with self.assertRaises(AssertionError):
            validate(weakened, script, source)

    def test_cannot_drop_extracted_smoke(self) -> None:
        workflow = WORKFLOW.read_text(encoding="utf-8")
        script = SCRIPT.read_text(encoding="utf-8")
        source = SOURCE.read_text(encoding="utf-8")
        weakened = script.replace("& $freshExe convert $fixture --to pdf", "& $built convert $fixture --to pdf")
        with self.assertRaises(AssertionError):
            validate(workflow, weakened, source)

    def test_cannot_claim_signed_release(self) -> None:
        workflow = WORKFLOW.read_text(encoding="utf-8")
        script = SCRIPT.read_text(encoding="utf-8")
        source = SOURCE.read_text(encoding="utf-8")
        with self.assertRaises(AssertionError):
            validate(workflow.replace("contents: read", "contents: write"), script, source)


if __name__ == "__main__":
    unittest.main()
