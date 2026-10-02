param(
    [Parameter(Mandatory = $true)][string]$PacketPath,
    [Parameter(Mandatory = $true)][string]$OutputRoot
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$ExpectedExperiment = "CARLTON-PARAGRAPH-ALIGN-RECONCILE-01"
$ExpectedFixtureSha256 = "bf9cda0f632b5820ab9dbdbe1b838b2a988b2f3fdd69253c22b4fc3aef9f11c3"
$ExpectedPublisherExeSha256 = "e1ef8811b85b82045f37c4173b92726101be3a25e550b0dcb9f178df834ab20b"
$TargetText = "Reception"
$PbParagraphAlignmentCenter = 1
$PbParagraphAlignmentRight = 2

$packet = Get-Content -LiteralPath $PacketPath -Raw | ConvertFrom-Json
if ([string]$packet.id -ne $ExpectedExperiment) {
    throw "Unexpected experiment id: $($packet.id)"
}
if ([string]::IsNullOrWhiteSpace([string]$env:PUB_RESEARCH_FIXTURE)) {
    throw "PUB_RESEARCH_FIXTURE was not resolved by prepare_native_run.ps1."
}
if ([string]::IsNullOrWhiteSpace([string]$env:PUB_RESEARCH_PUBLISHER_EXE)) {
    throw "PUB_RESEARCH_PUBLISHER_EXE was not resolved by prepare_native_run.ps1."
}

$fixtureHash = (Get-FileHash -LiteralPath $env:PUB_RESEARCH_FIXTURE -Algorithm SHA256).Hash.ToLowerInvariant()
if ($fixtureHash -ne $ExpectedFixtureSha256) {
    throw "Exact Carlton fixture mismatch: $fixtureHash"
}
$publisherHash = (Get-FileHash -LiteralPath $env:PUB_RESEARCH_PUBLISHER_EXE -Algorithm SHA256).Hash.ToLowerInvariant()
if ($publisherHash -ne $ExpectedPublisherExeSha256) {
    throw "Publisher executable SHA-256 mismatch: $publisherHash"
}

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "../../..")).Path
Import-Module (Join-Path $repoRoot "tools/windows/pub-runtime/PubRuntime.psm1") -Force

$analysisDir = Join-Path $OutputRoot "analysis"
$logDir = Join-Path $OutputRoot "logs"
$privateDir = Join-Path $OutputRoot "private/carlton-paragraph-align-reconcile-01"
New-Item -ItemType Directory -Force -Path $analysisDir,$logDir,$privateDir | Out-Null

function Release-Com($Value) {
    if ($null -ne $Value -and [System.Runtime.InteropServices.Marshal]::IsComObject($Value)) {
        try { [void][System.Runtime.InteropServices.Marshal]::FinalReleaseComObject($Value) } catch {}
    }
}

function Close-Document($Document) {
    if ($null -eq $Document) { return }
    try { $Document.Saved = $true } catch {}
    try { $Document.Close() } catch {}
    Release-Com $Document
}

function Find-TargetLocation {
    param([Parameter(Mandatory = $true)]$Document)

    $matches = @()
    for ($pageIndex = 1; $pageIndex -le [int]$Document.Pages.Count; $pageIndex++) {
        $page = $null
        try {
            $page = $Document.Pages.Item($pageIndex)
            for ($shapeIndex = 1; $shapeIndex -le [int]$page.Shapes.Count; $shapeIndex++) {
                $shape = $null
                $textFrame = $null
                $textRange = $null
                try {
                    $shape = $page.Shapes.Item($shapeIndex)
                    try {
                        $textFrame = $shape.TextFrame
                        $textRange = $textFrame.TextRange
                        $text = [string]$textRange.Text
                        $normalized = $text.TrimEnd([char[]]([char]13,[char]10))
                        if ($normalized -eq $TargetText) {
                            $matches += [pscustomobject]@{
                                page_index = $pageIndex
                                shape_index = $shapeIndex
                            }
                        }
                    }
                    catch {
                        # Non-text shapes are expected and are not candidates.
                    }
                }
                finally {
                    Release-Com $textRange
                    Release-Com $textFrame
                    Release-Com $shape
                }
            }
        }
        finally {
            Release-Com $page
        }
    }

    if ($matches.Count -ne 1) {
        throw "Expected exactly one target text frame in exact Carlton source; found $($matches.Count)."
    }
    return $matches[0]
}

function Get-TargetSnapshot {
    param(
        [Parameter(Mandatory = $true)]$Document,
        [Parameter(Mandatory = $true)][string]$Phase
    )

    $location = Find-TargetLocation -Document $Document
    $page = $null
    $shape = $null
    $textFrame = $null
    $textRange = $null
    $paragraph = $null
    try {
        $page = $Document.Pages.Item([int]$location.page_index)
        $shape = $page.Shapes.Item([int]$location.shape_index)
        $textFrame = $shape.TextFrame
        $textRange = $textFrame.TextRange
        $paragraph = $textRange.ParagraphFormat

        return [ordered]@{
            phase = $Phase
            target_match_count = 1
            text_length = Get-PubSafeValue { [int]$textRange.Length } "TextRange.Length"
            alignment = Get-PubSafeValue { [int]$paragraph.Alignment } "ParagraphFormat.Alignment"
        }
    }
    finally {
        Release-Com $paragraph
        Release-Com $textRange
        Release-Com $textFrame
        Release-Com $shape
        Release-Com $page
    }
}

function Set-TargetAlignment {
    param(
        [Parameter(Mandatory = $true)]$Document,
        [Parameter(Mandatory = $true)][int]$Value
    )

    $location = Find-TargetLocation -Document $Document
    $page = $null
    $shape = $null
    $textFrame = $null
    $textRange = $null
    $paragraph = $null
    try {
        $page = $Document.Pages.Item([int]$location.page_index)
        $shape = $page.Shapes.Item([int]$location.shape_index)
        $textFrame = $shape.TextFrame
        $textRange = $textFrame.TextRange
        $paragraph = $textRange.ParagraphFormat
        $paragraph.Alignment = $Value
    }
    finally {
        Release-Com $paragraph
        Release-Com $textRange
        Release-Com $textFrame
        Release-Com $shape
        Release-Com $page
    }
}

function Invoke-Arm {
    param(
        [Parameter(Mandatory = $true)][string]$Name,
        $RequestedAlignment
    )

    $armDir = Join-Path $privateDir $Name
    New-Item -ItemType Directory -Force -Path $armDir | Out-Null
    $working = Join-Path $armDir "working.pub"
    Copy-Item -LiteralPath $env:PUB_RESEARCH_FIXTURE -Destination $working -Force

    $beforeFile = Get-PubFileRecord -Path $working
    $app = $null
    $doc = $null
    $publisher = $null
    $before = $null
    $afterMutation = $null
    $afterSave = $null
    try {
        $app = New-PubPublisherApplication
        $publisher = [ordered]@{
            version = [string]$app.Version
            build = [string]$app.Build
        }
        $doc = $app.Open($working, $false, $false)
        $before = Get-TargetSnapshot -Document $doc -Phase "before"

        if ($null -ne $RequestedAlignment) {
            Set-TargetAlignment -Document $doc -Value ([int]$RequestedAlignment)
        }
        $afterMutation = Get-TargetSnapshot -Document $doc -Phase "after_mutation"

        $doc.Save()
        $afterSave = Get-TargetSnapshot -Document $doc -Phase "after_save"
    }
    finally {
        Close-Document $doc
        Close-PubPublisherApplication $app
    }

    $afterFile = Get-PubFileRecord -Path $working
    $app2 = $null
    $doc2 = $null
    $fresh = $null
    try {
        $app2 = New-PubPublisherApplication
        $doc2 = $app2.Open($working, $true, $false)
        $fresh = Get-TargetSnapshot -Document $doc2 -Phase "fresh_reopen"
    }
    finally {
        Close-Document $doc2
        Close-PubPublisherApplication $app2
    }

    return [ordered]@{
        arm = $Name
        requested_alignment = if ($null -eq $RequestedAlignment) { $null } else { [int]$RequestedAlignment }
        publisher = $publisher
        before = $before
        after_mutation = $afterMutation
        after_save = $afterSave
        fresh_reopen = $fresh
        file = [ordered]@{
            pre_save_sha256 = [string]$beforeFile.sha256
            post_save_sha256 = [string]$afterFile.sha256
            post_save_size = [long]$afterFile.size
            changed = ([string]$beforeFile.sha256 -ne [string]$afterFile.sha256)
            private_pub_retained = $true
        }
    }
}

$control = Invoke-Arm -Name "control" -RequestedAlignment $null
$center = Invoke-Arm -Name "explicit-center" -RequestedAlignment $PbParagraphAlignmentCenter
$right = Invoke-Arm -Name "explicit-right" -RequestedAlignment $PbParagraphAlignmentRight

function Snapshot-AlignmentValue($Snapshot) {
    if ($null -eq $Snapshot -or $Snapshot.alignment.state -ne "value") { return $null }
    return [int]$Snapshot.alignment.value
}

$centerRoundtrip = (Snapshot-AlignmentValue $center.fresh_reopen) -eq $PbParagraphAlignmentCenter
$rightRoundtrip = (Snapshot-AlignmentValue $right.fresh_reopen) -eq $PbParagraphAlignmentRight
$controlBefore = Snapshot-AlignmentValue $control.before
$controlFresh = Snapshot-AlignmentValue $control.fresh_reopen
$controlStable = ($null -ne $controlBefore -and $controlBefore -eq $controlFresh)

$verdict = if ($centerRoundtrip -and $rightRoundtrip) {
    "semantic-center-right-roundtrip-confirmed"
} else {
    "inconclusive"
}

$result = [ordered]@{
    schema = "chaptera.carlton-paragraph-align-reconcile-01.v1"
    experiment_id = $ExpectedExperiment
    fixture = [ordered]@{
        sha256 = $fixtureHash
        size = [int64](Get-Item -LiteralPath $env:PUB_RESEARCH_FIXTURE).Length
    }
    publisher = [ordered]@{
        exe_sha256 = $publisherHash
        expected_version_prefix = "16.0.12527."
    }
    semantic_constants = [ordered]@{
        center = $PbParagraphAlignmentCenter
        right = $PbParagraphAlignmentRight
    }
    control = $control
    explicit_center = $center
    explicit_right = $right
    guards = [ordered]@{
        center_roundtrip = [bool]$centerRoundtrip
        right_roundtrip = [bool]$rightRoundtrip
        control_effective_alignment_stable_after_noop_save = [bool]$controlStable
    }
    verdict = $verdict
    evidence_boundary = "This native stage records exact Carlton Publisher COM effective paragraph alignment and produces same-base control/Center/Right private PUB outputs after Save and fresh reopen. Raw FDPP/STSH carrier interpretation is intentionally deferred to structural analysis of those retained private outputs; no PDF-derived alignment semantics are used."
}

Write-PubJson -Value $result -Path (Join-Path $analysisDir "carlton-paragraph-align-reconcile-01.json")
@(
    "experiment=$ExpectedExperiment",
    "fixture_sha256=$fixtureHash",
    "control_before_alignment=$controlBefore",
    "control_fresh_alignment=$controlFresh",
    "control_alignment_stable=$controlStable",
    "center_fresh_alignment=$(Snapshot-AlignmentValue $center.fresh_reopen)",
    "right_fresh_alignment=$(Snapshot-AlignmentValue $right.fresh_reopen)",
    "center_roundtrip=$centerRoundtrip",
    "right_roundtrip=$rightRoundtrip",
    "control_output_sha256=$($control.file.post_save_sha256)",
    "center_output_sha256=$($center.file.post_save_sha256)",
    "right_output_sha256=$($right.file.post_save_sha256)",
    "verdict=$verdict"
) | Set-Content -LiteralPath (Join-Path $logDir "carlton-paragraph-align-reconcile-01.txt") -Encoding ASCII

if ($verdict -eq "inconclusive") {
    throw "Carlton paragraph-alignment native semantic arms did not roundtrip; inspect private evidence."
}
