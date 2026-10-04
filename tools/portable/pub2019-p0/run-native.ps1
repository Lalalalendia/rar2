Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$ExpectedPublisherVersion = "16.0"
$ExpectedPublisherBuild = "12527"
$ExpectedPublisherFileVersion = "16.0.12527.22145"
$ExpectedPublisherExeSha256 = "e1ef8811b85b82045f37c4173b92726101be3a25e550b0dcb9f178df834ab20b"
$StackSha = "0ca858ed4806e81da2964d75d54d25a2ac0c6126074e9f82ea33b87701de4ade"
$Surface029Sha = "c0688f73b9bf8fc7677a1eecaadd30fa00fc1813f6b12dc39b8aa5eac973f81e"
$PbFilePublication = 1

function Get-Sha256([string]$Path) {
    return (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
}

function Release-Com($Value) {
    if ($null -ne $Value -and [Runtime.InteropServices.Marshal]::IsComObject($Value)) {
        try { [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($Value) } catch {}
    }
}

function Write-Json($Value, [string]$Path) {
    $parent = Split-Path -Parent $Path
    if ($parent) { New-Item -ItemType Directory -Force -Path $parent | Out-Null }
    $Value | ConvertTo-Json -Depth 16 | Set-Content -LiteralPath $Path -Encoding UTF8
}

function Assert-NotElevated {
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    $principal = New-Object Security.Principal.WindowsPrincipal($identity)
    if ($principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
        throw "Do not run this bundle as Administrator. The proven T866 path is the current interactive user session."
    }
}

function Assert-BundleManifest([string]$Root) {
    $manifestPath = Join-Path $Root "bundle-manifest.json"
    if (-not (Test-Path -LiteralPath $manifestPath -PathType Leaf)) { throw "bundle-manifest.json missing" }
    $manifest = Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
    if ([string]$manifest.schema -ne "chaptera.pub2019-portable-p0.v1") { throw "Unexpected bundle manifest schema." }
    foreach ($item in @($manifest.files)) {
        $relative = [string]$item.path
        $candidate = Join-Path $Root ($relative.Replace("/", "\"))
        if (-not (Test-Path -LiteralPath $candidate -PathType Leaf)) { throw "Bundle file missing: $relative" }
        $size = (Get-Item -LiteralPath $candidate).Length
        if ([int64]$size -ne [int64]$item.bytes) { throw "Bundle byte-length mismatch: $relative" }
        $actual = Get-Sha256 $candidate
        if ($actual -ne [string]$item.sha256) { throw "Bundle SHA-256 mismatch: $relative" }
    }
    return $manifest
}

function Assert-Publisher2019([string]$Root) {
    $runtime = Join-Path $Root "tools\windows\pub-runtime\PubRuntime.psm1"
    Import-Module $runtime -Force
    $publisher = Get-PubPublisherIdentity
    if (-not $publisher.available) { throw "Publisher COM is unavailable." }
    if ($publisher.version.state -ne "value" -or [string]$publisher.version.value -ne $ExpectedPublisherVersion) { throw "Publisher Version mismatch." }
    if ($publisher.build.state -ne "value" -or [string]$publisher.build.value -ne $ExpectedPublisherBuild) { throw "Publisher Build mismatch." }
    if ($publisher.path.state -ne "value") { throw "Publisher executable directory is unavailable." }
    $exe = Join-Path ([string]$publisher.path.value) "MSPUB.EXE"
    if (-not (Test-Path -LiteralPath $exe -PathType Leaf)) { throw "MSPUB.EXE missing at COM-reported path." }
    $fv = [Diagnostics.FileVersionInfo]::GetVersionInfo($exe).FileVersion
    if ([string]$fv -ne $ExpectedPublisherFileVersion) { throw "MSPUB.EXE file version mismatch: $fv" }
    $sha = Get-Sha256 $exe
    if ($sha -ne $ExpectedPublisherExeSha256) { throw "MSPUB.EXE SHA-256 mismatch: $sha" }
    return [ordered]@{ version = $ExpectedPublisherVersion; build = $ExpectedPublisherBuild; file_version = $fv; exe_sha256 = $sha }
}

function Assert-NoPublisherProcess {
    $p = @(Get-Process -Name MSPUB -ErrorAction SilentlyContinue)
    if ($p.Count -gt 0) { throw "MSPUB.EXE is already running. Close Publisher normally and run again." }
}

function New-NativeBlank {
    param([string]$Root, [string]$Path)
    Import-Module (Join-Path $Root "tools\windows\pub-runtime\PubRuntime.psm1") -Force
    $app = $null; $doc = $null
    try {
        $app = New-PubPublisherApplication
        try { $doc = $app.NewDocument() } catch { $doc = $app.Documents.Add() }
        if ($null -eq $doc) { throw "Publisher did not create a new document." }
        if ([int]$doc.Pages.Count -ne 1) { throw "Native blank must start with exactly one page." }
        if ([int]$doc.Pages.Item(1).Shapes.Count -ne 0) { throw "Native blank must start with zero shapes." }
        $parent = Split-Path -Parent $Path
        New-Item -ItemType Directory -Force -Path $parent | Out-Null
        $doc.SaveAs($Path, $PbFilePublication, $false)
    } finally {
        if ($null -ne $doc) { try { $doc.Saved = $true } catch {}; try { $doc.Close() } catch {}; Release-Com $doc }
        if ($null -ne $app) { try { Close-PubPublisherApplication $app } catch {} }
    }
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) { throw "Native blank SaveAs did not create a PUB." }
    return [ordered]@{ sha256 = Get-Sha256 $Path; bytes = (Get-Item -LiteralPath $Path).Length; provenance = "publisher-new-document-portable-seed" }
}

function Invoke-PowerShellStep {
    param([string]$Label, [string]$Script, [string[]]$Arguments, [string]$ConsoleLog)
    Write-Host ""
    Write-Host ("=" * 72)
    Write-Host $Label
    Write-Host ("=" * 72)
    & powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File $Script @Arguments *>&1 | Tee-Object -FilePath $ConsoleLog -Append
    if ($LASTEXITCODE -ne 0) { throw "$Label failed with exit code $LASTEXITCODE" }
}

function Copy-SafeTree {
    param([string]$SourceRoot, [string]$DestRoot)
    New-Item -ItemType Directory -Force -Path $DestRoot | Out-Null
    foreach ($name in @("analysis","logs")) {
        $src = Join-Path $SourceRoot $name
        if (Test-Path -LiteralPath $src) { Copy-Item -LiteralPath $src -Destination $DestRoot -Recurse -Force }
    }
    foreach ($name in @("environment.json","evidence-manifest.json")) {
        $src = Join-Path $SourceRoot $name
        if (Test-Path -LiteralPath $src -PathType Leaf) { Copy-Item -LiteralPath $src -Destination (Join-Path $DestRoot $name) -Force }
    }
}

$Root = $PSScriptRoot
$stamp = [DateTime]::UtcNow.ToString("yyyyMMdd-HHmmss")
$WorkRoot = Join-Path $Root ("work-" + $stamp)
$ReturnRoot = Join-Path $Root ("return-" + $stamp)
$TopLog = Join-Path $ReturnRoot "RUN_NATIVE.log"
New-Item -ItemType Directory -Force -Path $WorkRoot,$ReturnRoot | Out-Null

$summary = [ordered]@{
    schema = "chaptera.pub2019-portable-p0-run.v1"
    started_at_utc = [DateTime]::UtcNow.ToString("o")
    bundle = $null
    publisher = $null
    tasks = @()
    final_status = "running"
}

try { Start-Transcript -LiteralPath $TopLog -Force | Out-Null } catch {}

try {
    Assert-NotElevated
    Assert-NoPublisherProcess
    $manifest = Assert-BundleManifest $Root
    $summary.bundle = [ordered]@{ source_main_sha = [string]$manifest.source_main_sha; corpus_artifact_id = [int64]$manifest.corpus_artifact_id }
    $summary.publisher = Assert-Publisher2019 $Root

    $stackFixture = Join-Path $Root ("fixtures\" + $StackSha + ".pub")
    $surfaceFixture = Join-Path $Root ("fixtures\" + $Surface029Sha + ".pub")
    if ((Get-Sha256 $stackFixture) -ne $StackSha) { throw "033 fixture SHA mismatch." }
    if ((Get-Sha256 $surfaceFixture) -ne $Surface029Sha) { throw "029 fixture SHA mismatch." }

    # Embedded Python is carried as a pinned ZIP and expanded only into this run work directory.
    $pythonZip = Join-Path $Root "runtime\python-3.13.16-embed-amd64.zip"
    $pythonRoot = Join-Path $WorkRoot "python"
    Expand-Archive -LiteralPath $pythonZip -DestinationPath $pythonRoot -Force
    $python = Join-Path $pythonRoot "python.exe"
    if (-not (Test-Path -LiteralPath $python -PathType Leaf)) { throw "Embedded python.exe missing after extraction." }

    # 033
    $stackOut = Join-Path $WorkRoot "033-stack"
    $stackConsole = Join-Path $ReturnRoot "033-stack-console.txt"
    try {
        Invoke-PowerShellStep "033 MASTER/PAGE STACK" (Join-Path $Root "tools\research-runner\operations\master_projection_stack_auth_01.ps1") @("-InputPath",$stackFixture,"-OutputRoot",$stackOut) $stackConsole
        $receipt = Get-Content -LiteralPath (Join-Path $stackOut "analysis\master-projection-stack-auth-01.json") -Raw | ConvertFrom-Json
        $summary.tasks += [ordered]@{ id="033-stack"; status="success"; verdict=[string]$receipt.verdict }
        Copy-SafeTree $stackOut (Join-Path $ReturnRoot "033-stack")
    } catch {
        $summary.tasks += [ordered]@{ id="033-stack"; status="failed"; error=$_.Exception.Message }
    }
    Assert-NoPublisherProcess

    # 029
    $surfaceOut = Join-Path $WorkRoot "029-surface"
    $surfaceConsole = Join-Path $ReturnRoot "029-surface-console.txt"
    try {
        Invoke-PowerShellStep "029 NATIVE PAGE/SPREAD" (Join-Path $Root "tools\research-runner\operations\mature_029_native_page_spread_oracle_01.ps1") @("-InputPath",$surfaceFixture,"-OutputRoot",$surfaceOut) $surfaceConsole
        $native029 = Join-Path $surfaceOut "analysis\mature-029-native-page-spread-oracle-01.json"
        $class029 = Join-Path $surfaceOut "analysis\mature-029-reference-surface-classification.json"
        & $python (Join-Path $Root "tools\classify_mature_029_native_page_spread_v1.py") $native029 --out $class029
        if ($LASTEXITCODE -ne 0) { throw "029 classifier failed with exit code $LASTEXITCODE" }
        $classified = Get-Content -LiteralPath $class029 -Raw | ConvertFrom-Json
        $summary.tasks += [ordered]@{ id="029-surface"; status="success"; reference_surface_stage=[string]$classified.reference_surface_stage }
        Copy-SafeTree $surfaceOut (Join-Path $ReturnRoot "029-surface")
    } catch {
        $summary.tasks += [ordered]@{ id="029-surface"; status="failed"; error=$_.Exception.Message }
    }
    Assert-NoPublisherProcess

    # Paragraph metrics portable variant: Publisher itself creates the zero-shape blank.
    $paragraphOut = Join-Path $WorkRoot "paragraph-metrics"
    $paragraphConsole = Join-Path $ReturnRoot "paragraph-metrics-console.txt"
    try {
        New-Item -ItemType Directory -Force -Path $paragraphOut | Out-Null
        $blank = Join-Path $WorkRoot "paragraph-native-blank.pub"
        $blankIdentity = New-NativeBlank -Root $Root -Path $blank
        $env:PUB_RESEARCH_FIXTURE = $blank
        $packet = Join-Path $Root "tools\research-runner\experiments\paragraph-metrics-auth-01.packet.json"
        Invoke-PowerShellStep "PARAGRAPH METRICS NATIVE MATRIX" (Join-Path $Root "tools\research-runner\operations\paragraph_metrics_auth_01.ps1") @("-PacketPath",$packet,"-OutputRoot",$paragraphOut) $paragraphConsole

        $envDoc = [ordered]@{
            schema = "chaptera.pub2019-portable-paragraph-environment.v1"
            experiment_id = "PARAGRAPH-METRICS-AUTH-01"
            publisher = $summary.publisher
            fixture = $blankIdentity
            authority_note = "Portable variant uses Publisher.NewDocument/Documents.Add native blank rather than historical 5bf605... profile-instantiated blank. Common-seed 11-arm causal matrix remains exact after seed creation."
        }
        Write-Json $envDoc (Join-Path $paragraphOut "environment.json")

        & $python (Join-Path $Root "tools\research-runner\analysis\paragraph_metrics_auth_01_blast_radius.py") --output-root $paragraphOut
        if ($LASTEXITCODE -ne 0) { throw "paragraph blast-radius analysis failed with exit code $LASTEXITCODE" }
        $probe = Join-Path $Root "runtime\paragraph-metrics-probe.exe"
        & $python (Join-Path $Root "tools\research-runner\analysis\paragraph_metrics_auth_01_structural.py") --output-root $paragraphOut --snapshot-tool $probe
        if ($LASTEXITCODE -ne 0) { throw "paragraph structural analysis failed with exit code $LASTEXITCODE" }
        Invoke-PowerShellStep "PARAGRAPH EVIDENCE FINALIZE" (Join-Path $Root "tools\research-runner\finalize_native_run.ps1") @("-PacketPath",$packet,"-OutputRoot",$paragraphOut) $paragraphConsole

        $structural = Get-Content -LiteralPath (Join-Path $paragraphOut "analysis\paragraph-metrics-auth-01-structural.json") -Raw | ConvertFrom-Json
        $summary.tasks += [ordered]@{ id="paragraph-metrics"; status="success"; text_invariance=[bool]$structural.invariants.quill_text_byte_invariance_all_arms; semantics_granted=[bool]$structural.invariants.paragraph_metric_semantics_granted; seed_provenance=[string]$blankIdentity.provenance }
        Copy-SafeTree $paragraphOut (Join-Path $ReturnRoot "paragraph-metrics")
    } catch {
        $summary.tasks += [ordered]@{ id="paragraph-metrics"; status="failed"; error=$_.Exception.Message }
    }

    $ok = @($summary.tasks | Where-Object { $_.status -eq "success" }).Count
    $summary.final_status = if ($ok -eq 3) { "all_three_success" } elseif ($ok -gt 0) { "partial_success" } else { "failed" }
} catch {
    $summary.final_status = "bootstrap_failed"
    $summary.bootstrap_error = $_.Exception.Message
} finally {
    $summary.finished_at_utc = [DateTime]::UtcNow.ToString("o")
    Write-Json $summary (Join-Path $ReturnRoot "RUN-SUMMARY.json")
    Copy-Item -LiteralPath (Join-Path $Root "bundle-manifest.json") -Destination (Join-Path $ReturnRoot "bundle-manifest.json") -Force
    try { Stop-Transcript | Out-Null } catch {}
    $returnZip = Join-Path $Root ("RETURN-TO-CHAT-" + $stamp + ".zip")
    try { Compress-Archive -Path (Join-Path $ReturnRoot "*") -DestinationPath $returnZip -CompressionLevel Optimal -Force } catch {}
    Write-Host ""
    Write-Host "FINAL STATUS: $($summary.final_status)"
    Write-Host "RETURN ZIP: $returnZip"
}

if ($summary.final_status -eq "all_three_success") { exit 0 } else { exit 1 }
