param(
    [Parameter(Mandatory = $true)][string]$PacketPath,
    [Parameter(Mandatory = $true)][string]$OutputRoot
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$ExpectedExperiment = "VBA-NATIVE-POSITIVE-2019-01"
$PbFilePublication = 1
$VbextCtStdModule = 1

Import-Module (Join-Path (Split-Path -Parent (Split-Path -Parent $PSScriptRoot)) "windows\pub-runtime\PubRuntime.psm1") -Force

$packet = Get-Content -LiteralPath $PacketPath -Raw | ConvertFrom-Json
if ([string]$packet.id -ne $ExpectedExperiment) { throw "Unexpected experiment id: $($packet.id)" }
if ([string]::IsNullOrWhiteSpace([string]$env:PUB_RESEARCH_FIXTURE)) { throw "PUB_RESEARCH_FIXTURE missing" }

$analysisDir = Join-Path $OutputRoot "analysis"
$logDir = Join-Path $OutputRoot "logs"
$privateDir = Join-Path $OutputRoot "private\vba-native-positive-2019-01"
New-Item -ItemType Directory -Force -Path $analysisDir,$logDir,$privateDir | Out-Null

$resultPath = Join-Path $analysisDir "vba-native-positive-2019-01.json"
$scanPath = Join-Path $analysisDir "vba-native-positive-scan.json"
$logPath = Join-Path $logDir "vba-native-positive-2019-01.txt"
$input = Join-Path $privateDir "input.pub"
$output = Join-Path $privateDir "native-positive.pub"
Copy-Item -LiteralPath $env:PUB_RESEARCH_FIXTURE -Destination $input -Force
if (Test-Path -LiteralPath $output) { Remove-Item -LiteralPath $output -Force }

$macroSource = @"
Option Explicit
Public Sub ChapteraProbe()
    Dim x As Long
    x = 1
End Sub
"@
$macroSourceSha256 = [Convert]::ToHexString(
    [Security.Cryptography.SHA256]::HashData([Text.Encoding]::UTF8.GetBytes($macroSource))
).ToLowerInvariant()

function Close-Document($doc) {
    if ($null -eq $doc) { return }
    try { $doc.Saved = $true } catch {}
    try { $doc.Close() } catch {}
    try { [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($doc) } catch {}
}

function Release-Com($obj) {
    if ($null -eq $obj) { return }
    try { [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($obj) } catch {}
}

function Get-ProjectSummary($project) {
    $name = $null
    $fileName = $null
    try { $name = [string]$project.Name } catch {}
    try { $fileName = [string]$project.FileName } catch {}
    return [ordered]@{
        name = $name
        file_name_leaf = if ([string]::IsNullOrWhiteSpace($fileName)) { $null } else { [IO.Path]::GetFileName($fileName) }
    }
}

function Write-ResultAndLog($value, [string[]]$lines) {
    Write-PubJson -Value $value -Path $resultPath
    $lines | Set-Content -LiteralPath $logPath -Encoding ASCII
}

$app = $null
$doc = $null
$vbe = $null
$project = $null
$component = $null
$codeModule = $null
$accessPath = $null

try {
    $app = New-PubPublisherApplication
    $doc = $app.Open($input, $false, $false)
    try { $doc.Activate() } catch {}

    # Prefer a document-bound project if Publisher exposes one. Otherwise use
    # the VBE active project after explicitly activating this document.
    try {
        $candidate = $doc.VBProject
        if ($null -ne $candidate) {
            $project = $candidate
            $accessPath = "Document.VBProject"
        }
    } catch {}

    if ($null -eq $project) {
        try {
            $vbe = $app.VBE
        } catch {
            $blocked = [ordered]@{
                schema = "pub-vba-native-positive-2019-01/v1"
                experiment_id = $ExpectedExperiment
                verdict = "blocked"
                blocker_code = "vbe_access_unavailable_or_untrusted"
                exception_type = $_.Exception.GetType().FullName
                macro_execution_invoked = $false
                trust_settings_modified = $false
                generated_pub_private_local = $false
                macro_source_sha256 = $macroSourceSha256
            }
            Write-ResultAndLog $blocked @(
                "experiment=$ExpectedExperiment",
                "verdict=blocked",
                "blocker=vbe_access_unavailable_or_untrusted",
                "macro_execution_invoked=false",
                "trust_settings_modified=false"
            )
            throw "Publisher VBE access is unavailable or blocked by Trust Center policy."
        }

        try {
            $candidate = $vbe.ActiveVBProject
            if ($null -ne $candidate) {
                $project = $candidate
                $accessPath = "Application.VBE.ActiveVBProject"
            }
        } catch {}
    }

    if ($null -eq $project) {
        $blocked = [ordered]@{
            schema = "pub-vba-native-positive-2019-01/v1"
            experiment_id = $ExpectedExperiment
            verdict = "blocked"
            blocker_code = "no_active_vbproject"
            macro_execution_invoked = $false
            trust_settings_modified = $false
            generated_pub_private_local = $false
            macro_source_sha256 = $macroSourceSha256
        }
        Write-ResultAndLog $blocked @(
            "experiment=$ExpectedExperiment",
            "verdict=blocked",
            "blocker=no_active_vbproject",
            "macro_execution_invoked=false",
            "trust_settings_modified=false"
        )
        throw "Publisher exposed no active VBProject for the opened publication."
    }

    # ActiveVBProject is a global VBE selection. Never mutate it unless it is
    # provably bound to the exact publication we opened for this experiment.
    if ($accessPath -eq "Application.VBE.ActiveVBProject") {
        $projectFileName = $null
        try { $projectFileName = [string]$project.FileName } catch {}
        $bound = $false
        if (-not [string]::IsNullOrWhiteSpace($projectFileName)) {
            try {
                $bound = [string]::Equals(
                    [IO.Path]::GetFullPath($projectFileName),
                    [IO.Path]::GetFullPath($input),
                    [StringComparison]::OrdinalIgnoreCase
                )
            } catch {
                $bound = $false
            }
        }
        if (-not $bound) {
            $blocked = [ordered]@{
                schema = "pub-vba-native-positive-2019-01/v1"
                experiment_id = $ExpectedExperiment
                verdict = "blocked"
                blocker_code = "active_vbproject_not_bound_to_fixture"
                vbproject_access_path = $accessPath
                active_project_file_leaf = if ([string]::IsNullOrWhiteSpace($projectFileName)) { $null } else { [IO.Path]::GetFileName($projectFileName) }
                expected_file_leaf = [IO.Path]::GetFileName($input)
                macro_execution_invoked = $false
                trust_settings_modified = $false
                generated_pub_private_local = $false
                macro_source_sha256 = $macroSourceSha256
            }
            Write-ResultAndLog $blocked @(
                "experiment=$ExpectedExperiment",
                "verdict=blocked",
                "blocker=active_vbproject_not_bound_to_fixture",
                "macro_execution_invoked=false",
                "trust_settings_modified=false"
            )
            throw "ActiveVBProject is not provably bound to the exact input publication."
        }
    }

    $projectSummaryBefore = Get-ProjectSummary $project
    $component = $project.VBComponents.Add($VbextCtStdModule)
    $component.Name = "ChapteraProbe"
    $codeModule = $component.CodeModule
    $codeModule.AddFromString($macroSource)

    # Save only. Never call Application.Run, MacroOptions, or any macro execution surface.
    $doc.SaveAs($output, $PbFilePublication, $false)
    if (-not (Test-Path -LiteralPath $output -PathType Leaf)) {
        throw "Publisher did not materialize the macro-positive PUB."
    }

    $generated = Get-Item -LiteralPath $output
    $generatedSha256 = (Get-FileHash -LiteralPath $output -Algorithm SHA256).Hash.ToLowerInvariant()
    $projectSummaryAfter = Get-ProjectSummary $project
}
finally {
    Release-Com $codeModule
    Release-Com $component
    Release-Com $project
    Release-Com $vbe
    Close-Document $doc
    Close-PubPublisherApplication $app
}

python tools/pub_vba_estate_scan.py scan --input $output --output $scanPath
if ($LASTEXITCODE -ne 0) { throw "Inert VBA scanner failed with exit code $LASTEXITCODE" }

$scan = Get-Content -LiteralPath $scanPath -Raw | ConvertFrom-Json
if ([int]$scan.totals.file_count -ne 1) { throw "Expected one scanned native-positive PUB." }
$row = @($scan.files)[0]
$admittedProjects = [int]$scan.totals.vba_project_present_any
$extractedSources = 0
$sourceFailures = 0
foreach ($p in @($row.vba_projects)) {
    if ([bool]$p.structural_valid) {
        $extractedSources += [int]$p.module_sources_extracted
        $sourceFailures += [int]$p.module_source_failures
    }
}

$pass = (
    [int]$scan.totals.cfb_ok -eq 1 -and
    $admittedProjects -eq 1 -and
    $extractedSources -ge 1 -and
    [bool]$scan.claims.vba_executed -eq $false -and
    [bool]$scan.claims.ole_com_activated -eq $false -and
    [bool]$scan.claims.source_text_emitted -eq $false
)

$result = [ordered]@{
    schema = "pub-vba-native-positive-2019-01/v1"
    experiment_id = $ExpectedExperiment
    verdict = if ($pass) { "native-positive-confirmed" } else { "scanner-positive-path-inconclusive" }
    vbproject_access_path = $accessPath
    project_before = $projectSummaryBefore
    project_after = $projectSummaryAfter
    macro_source_sha256 = $macroSourceSha256
    macro_execution_invoked = $false
    trust_settings_modified = $false
    generated_pub = [ordered]@{
        sha256 = $generatedSha256
        size = [int64]$generated.Length
        private_local = $true
    }
    scanner = [ordered]@{
        cfb_ok = [int]$scan.totals.cfb_ok
        vba_storage_seen_any = [int]$scan.totals.vba_storage_seen_any
        vba_project_present_any = $admittedProjects
        vba_state = [string]$row.vba_state
        module_sources_extracted = $extractedSources
        module_source_failures = $sourceFailures
        call_family_hits = $scan.totals.call_family_hits
        symbol_hits = $scan.totals.symbol_hits
        vba_executed = [bool]$scan.claims.vba_executed
        ole_com_activated = [bool]$scan.claims.ole_com_activated
        source_text_emitted = [bool]$scan.claims.source_text_emitted
    }
}
Write-ResultAndLog $result @(
    "experiment=$ExpectedExperiment",
    "verdict=$($result.verdict)",
    "vbproject_access_path=$accessPath",
    "admitted_projects=$admittedProjects",
    "module_sources_extracted=$extractedSources",
    "module_source_failures=$sourceFailures",
    "macro_execution_invoked=false",
    "trust_settings_modified=false",
    "generated_pub_private_local=true"
)

if (-not $pass) {
    throw "Native Publisher file was created, but the inert scanner positive-path contract did not pass."
}
