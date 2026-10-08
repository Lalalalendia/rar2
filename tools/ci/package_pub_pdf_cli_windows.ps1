# Package only the already-built CLI. No source document, test PDF, or font enters the ZIP.
param(
    [Parameter(Mandatory = $true)]
    [ValidatePattern('^[0-9a-f]{40}$')]
    [string] $CandidateSha
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"

function Get-Sha256([string] $Path) {
    return (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
}

function Write-Utf8([string] $Path, [string] $Content) {
    [System.IO.File]::WriteAllText($Path, $Content, [System.Text.UTF8Encoding]::new($false))
}

function Assert-Same([string] $Label, [object] $Actual, [object] $Expected) {
    if ([string]$Actual -cne [string]$Expected) {
        throw "$Label mismatch: actual=$Actual expected=$Expected"
    }
}

$root = (Resolve-Path (Join-Path $PSScriptRoot "../..")).Path
$head = (& git -C $root rev-parse HEAD).Trim()
Assert-Same "checked out commit" $head $CandidateSha
$rustc = (& rustc --version).Trim()
if (-not $rustc.StartsWith("rustc 1.94.1 ")) {
    throw "unexpected Rust compiler: $rustc"
}

$built = Join-Path $root "vendor/producer-a/target/release/pub.exe"
if (-not (Test-Path -LiteralPath $built)) { throw "release pub.exe missing" }

$work = Join-Path $env:RUNNER_TEMP "pub-pdf-cli-package-work"
$publish = Join-Path $env:RUNNER_TEMP "pub-pdf-cli-package-output"
Remove-Item -Recurse -Force -ErrorAction SilentlyContinue $work, $publish
New-Item -ItemType Directory -Force -Path $work, $publish | Out-Null
$stage = Join-Path $work "Chaptera-PUB-PDF-CLI-Windows-x64"
New-Item -ItemType Directory -Force -Path $stage | Out-Null
$stageExe = Join-Path $stage "pub.exe"
Copy-Item -LiteralPath $built -Destination $stageExe
$exeSha = Get-Sha256 $stageExe
$exeLen = (Get-Item -LiteralPath $stageExe).Length
if ($exeLen -le 0) { throw "empty pub.exe" }

$readme = Join-Path $stage "README.txt"
$readmeText = @'
Chaptera PUB to PDF CLI - UNSIGNED TECHNICAL PREVIEW
Windows x64, standalone command-line executable. No installer or signing.

In PowerShell:
  .\pub.exe convert "input.pub" --to pdf --output "output.pdf" --fallback-font "licensed-font.ttf"

The TTF/TTC font file must be supplied by the user. The package does not
contain or select any fallback font automatically. It checks OpenType
embedding flags but cannot determine legal permission for the supplied font.

Current bounded route: mature 0x2C Publisher files. Output is PDF plus
output.pdf.loss.json and output.pdf.loss.txt. Read those loss reports.
No claim of Publisher-perfect visual fidelity, source-font identity,
PDF/A or PDF/X compatibility. This CLI is not a sandbox for untrusted files.

This is a 7-day GitHub Actions preview artifact, not a signed public
GitHub Release. Windows may warn about the unsigned program.
SHA256SUMS.txt identifies the exact bundled bytes.
'@
Write-Utf8 $readme ($readmeText.Trim() + [Environment]::NewLine)

$manifestFile = Join-Path $stage "manifest.json"
$manifest = [ordered]@{
    schema_version = "chaptera.pub-pdf-cli-windows-package.v1"
    candidate_sha = $CandidateSha
    target = "x86_64-pc-windows-msvc"
    profile = "release"
    rustc_version = $rustc
    binary_entry = "pub.exe"
    binary_sha256 = $exeSha
    binary_byte_len = $exeLen
    signature = "UNSIGNED_TECHNICAL_PREVIEW"
    distribution = "GITHUB_ACTIONS_ARTIFACT_ONLY"
}
Write-Utf8 $manifestFile (($manifest | ConvertTo-Json -Depth 5) + [Environment]::NewLine)

$sumsFile = Join-Path $stage "SHA256SUMS.txt"
$expectedSums = @(
    "$(Get-Sha256 $stageExe)  pub.exe"
    "$(Get-Sha256 $readme)  README.txt"
    "$(Get-Sha256 $manifestFile)  manifest.json"
) -join [Environment]::NewLine
Write-Utf8 $sumsFile ($expectedSums + [Environment]::NewLine)

$zipName = "Chaptera-PUB-PDF-CLI-Windows-x64.zip"
$zip = Join-Path $publish $zipName
Compress-Archive -LiteralPath $stage -DestinationPath $zip -CompressionLevel Optimal

# The shipped ZIP, not target/release/pub.exe, is the acceptance subject.
$extract = Join-Path $work "clean-consumer"
Expand-Archive -LiteralPath $zip -DestinationPath $extract
$unpacked = Join-Path $extract "Chaptera-PUB-PDF-CLI-Windows-x64"
if (-not (Test-Path -LiteralPath $unpacked)) { throw "ZIP root directory missing" }
$expectedFiles = @("README.txt", "SHA256SUMS.txt", "manifest.json", "pub.exe")
$actualFiles = @(Get-ChildItem -LiteralPath $unpacked -File | ForEach-Object { $_.Name } | Sort-Object)
$fileDifferences = @(Compare-Object -ReferenceObject ($expectedFiles | Sort-Object) -DifferenceObject $actualFiles)
if ($fileDifferences.Count -ne 0) {
    throw "ZIP contains missing or unexpected files"
}
if (@(Get-ChildItem -LiteralPath $unpacked -Recurse -Directory).Count -ne 0) {
    throw "ZIP contains unexpected directories"
}
$freshExe = Join-Path $unpacked "pub.exe"
$freshManifest = Get-Content -Raw -LiteralPath (Join-Path $unpacked "manifest.json") | ConvertFrom-Json
Assert-Same "shipped SHA" (Get-Sha256 $freshExe) $exeSha
Assert-Same "manifest binary SHA" $freshManifest.binary_sha256 $exeSha
Assert-Same "manifest candidate" $freshManifest.candidate_sha $CandidateSha
Assert-Same "manifest signing claim" $freshManifest.signature "UNSIGNED_TECHNICAL_PREVIEW"
$actualSums = Get-Content -Raw -LiteralPath (Join-Path $unpacked "SHA256SUMS.txt")
Assert-Same "shipped checksums" $actualSums ($expectedSums + [Environment]::NewLine)
Assert-Same "unpacked README SHA" (Get-Sha256 (Join-Path $unpacked "README.txt")) (Get-Sha256 $readme)
Assert-Same "unpacked manifest SHA" (Get-Sha256 (Join-Path $unpacked "manifest.json")) (Get-Sha256 $manifestFile)

# Test data stays outside the shipping tree and outside uploaded artifacts.
$sourceSha = "6a825ba26ba35d6e885acdc62e859591ed37cb0ff7480b554b9cb362b644dfcf"
$fontSha = "80307b8da7649aa4ee4d484b232140e3ce1ec0ca093073d3c53c8f5a5ced7a70"
$fixture = Join-Path $work "SampleNewsletter.pub"
Invoke-WebRequest -Uri "https://raw.githubusercontent.com/apache/poi/942d95d85b15d0dfdb3bc9ba1b4f273f277757c8/test-data/publisher/SampleNewsletter.pub" -OutFile $fixture
Assert-Same "fixture SHA256" (Get-Sha256 $fixture) $sourceSha
Assert-Same "fixture size" (Get-Item -LiteralPath $fixture).Length 291840

$font = Join-Path $work "chaptera-fallback.ttf"
Push-Location $root
try {
    & cargo run -q -p chaptera-desktop-fallback-font-resource --bin materialize-fallback-font -- $font
    if ($LASTEXITCODE -ne 0) { throw "fallback font materializer failed: $LASTEXITCODE" }
}
finally { Pop-Location }
if (-not (Test-Path -LiteralPath $font)) { throw "fallback font missing" }
Assert-Same "fallback font SHA256" (Get-Sha256 $font) $fontSha

$pdfA = Join-Path $work "a.pdf"
$pdfB = Join-Path $work "b.pdf"
$jsonA = Join-Path $work "a.cli.json"
$jsonB = Join-Path $work "b.cli.json"
& $freshExe convert $fixture --to pdf --output $pdfA --fallback-font $font --json > $jsonA
if ($LASTEXITCODE -ne 0) { throw "first extracted binary conversion failed: $LASTEXITCODE" }
& $freshExe convert $fixture --to pdf --output $pdfB --fallback-font $font --json > $jsonB
if ($LASTEXITCODE -ne 0) { throw "second extracted binary conversion failed: $LASTEXITCODE" }

foreach ($suffix in @("", ".loss.json", ".loss.txt")) {
    $left = "$pdfA$suffix"
    $right = "$pdfB$suffix"
    if (-not (Test-Path -LiteralPath $left) -or -not (Test-Path -LiteralPath $right)) {
        throw "required output missing: $suffix"
    }
    Assert-Same "repeat conversion $suffix SHA256" (Get-Sha256 $left) (Get-Sha256 $right)
}
$pdfBytes = [System.IO.File]::ReadAllBytes($pdfA)
if ($pdfBytes.Length -lt 8) { throw "PDF too small" }
Assert-Same "PDF header" ([System.Text.Encoding]::ASCII.GetString($pdfBytes, 0, 8)) "%PDF-1.7"
$report = Get-Content -Raw -LiteralPath "$pdfA.loss.json" | ConvertFrom-Json -Depth 100
$cliReport = Get-Content -Raw -LiteralPath $jsonA | ConvertFrom-Json -Depth 100
Assert-Same "loss report schema" $report.schema_version "free-pub-pdf-v0.1"
Assert-Same "source report label" $report.source.label "SampleNewsletter.pub"
Assert-Same "fallback report label" $report.typography.fallback_font.label "chaptera-fallback.ttf"
Assert-Same "CLI and sidecar schema" $cliReport.schema_version $report.schema_version
Assert-Same "source hash in report" $report.conversion_profile.source.source_sha256 $sourceSha
Assert-Same "target format" $report.target.format "pdf"
Assert-Same "typography disposition" $report.typography.disposition "explicit_user_fallback_not_source_font"
Assert-Same "font fingerprint" $report.typography.fallback_font.fingerprint_sha256 $fontSha
Assert-Same "output reshaping" $report.typography.shaped_flow.output_adapter_reshaping_calls 0
Assert-Same "fixed flow reshaping" $report.typography.fixed_flow_receipt.invariants.reshaping_calls 0
Assert-Same "raw text emitted" $report.typography.fixed_flow_receipt.invariants.raw_text_emitted $false
Assert-Same "line order preserved" $report.typography.fixed_flow_receipt.invariants.line_order_preserved $true
Assert-Same "resolved text capability" $report.pdf.capabilities.resolved_text $true
Assert-Same "image capability" $report.pdf.capabilities.images $true
if (@($report.typography.materialized_runs).Count -le 0) { throw "no materialized text runs" }
if (@($report.typography.skipped).Count -ne 0) { throw "pinned fixture skipped text runs" }

$zipSha = Get-Sha256 $zip
$receipt = [ordered]@{
    schema_version = "chaptera.pub-pdf-cli-windows-package-acceptance.v1"
    status = "PASS"
    candidate_sha = $CandidateSha
    target = "x86_64-pc-windows-msvc"
    zip_sha256 = $zipSha
    binary_sha256 = $exeSha
    source_sha256 = $sourceSha
    source_label = $report.source.label
    fallback_font_sha256 = $fontSha
    fallback_font_label = $report.typography.fallback_font.label
    pdf_sha256 = Get-Sha256 $pdfA
    loss_json_sha256 = Get-Sha256 "$pdfA.loss.json"
    loss_text_sha256 = Get-Sha256 "$pdfA.loss.txt"
    materialized_run_count = @($report.typography.materialized_runs).Count
    skipped_run_count = @($report.typography.skipped).Count
    extracted_binary_executed = $true
    deterministic_pdf_and_loss = $true
    exact_package_allowlist = $true
    packaged_font_or_source = $false
    signature = "UNSIGNED_TECHNICAL_PREVIEW"
}
Write-Utf8 (Join-Path $publish "acceptance.json") (($receipt | ConvertTo-Json -Depth 6) + [Environment]::NewLine)
Write-Utf8 (Join-Path $publish "SHA256SUMS.txt") ("$zipSha  $zipName" + [Environment]::NewLine)
Write-Host ("PASS: shipped Windows CLI SHA256 {0}; archive SHA256 {1}" -f $exeSha, $zipSha)
