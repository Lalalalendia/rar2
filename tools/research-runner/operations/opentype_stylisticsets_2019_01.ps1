param(
    [Parameter(Mandatory = $true)][string]$PacketPath,
    [Parameter(Mandatory = $true)][string]$OutputRoot
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$ExpectedExperiment = "OPENTYPE-STYLISTICSETS-2019-01"
$PbFilePublication = 1
$MsoTextOrientationHorizontal = 1

Import-Module (Join-Path (Split-Path -Parent (Split-Path -Parent $PSScriptRoot)) "windows\pub-runtime\PubRuntime.psm1") -Force

$packet = Get-Content -LiteralPath $PacketPath -Raw | ConvertFrom-Json
if ([string]$packet.id -ne $ExpectedExperiment) { throw "Unexpected experiment id: $($packet.id)" }
if ([string]::IsNullOrWhiteSpace([string]$env:PUB_RESEARCH_FIXTURE)) { throw "PUB_RESEARCH_FIXTURE missing" }

$analysisDir = Join-Path $OutputRoot "analysis"
$logDir = Join-Path $OutputRoot "logs"
$privateDir = Join-Path $OutputRoot "private\opentype-stylisticsets-2019-01"
New-Item -ItemType Directory -Force -Path $analysisDir,$logDir,$privateDir | Out-Null

function Close-Document($doc) {
    if ($null -eq $doc) { return }
    try { $doc.Saved = $true } catch {}
    try { $doc.Close() } catch {}
    try { [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($doc) } catch {}
}

function Get-FontSnapshot($shape, [string]$phase) {
    $font = $shape.TextFrame.TextRange.Font
    return [ordered]@{
        phase = $phase
        shape_id = Get-PubSafeValue { [int]$shape.ID } "Shape.ID"
        text = Get-PubSafeValue { [string]$shape.TextFrame.TextRange.Text } "TextRange.Text"
        font_name = Get-PubSafeValue { [string]$font.Name } "Font.Name"
        font_size = Get-PubSafeValue { [double]$font.Size } "Font.Size"
        stylistic_sets = Get-PubSafeValue { [int]$font.StylisticSets } "Font.StylisticSets"
        ligature = Get-PubSafeValue { [int]$font.Ligature } "Font.Ligature"
        contextual_alternates = Get-PubSafeValue { [int]$font.ContextualAlternates } "Font.ContextualAlternates"
    }
}

function Invoke-Arm([int]$value) {
    $armDir = Join-Path $privateDir ("ss-" + $value)
    New-Item -ItemType Directory -Force -Path $armDir | Out-Null
    $input = Join-Path $armDir "input.pub"
    $output = Join-Path $armDir "output.pub"
    Copy-Item -LiteralPath $env:PUB_RESEARCH_FIXTURE -Destination $input -Force

    $app = $null
    $doc = $null
    $before = $null
    try {
        $app = New-PubPublisherApplication
        $doc = $app.Open($input, $false, $false)
        $shape = $doc.Pages.Item(1).Shapes.AddTextbox($MsoTextOrientationHorizontal, 72, 72, 360, 120)
        $shape.TextFrame.TextRange.Text = "office affine stylistic alternate sample"
        $shape.TextFrame.TextRange.Font.Name = "Gabriola"
        $shape.TextFrame.TextRange.Font.Size = 24
        $shape.TextFrame.TextRange.Font.StylisticSets = $value
        $before = Get-FontSnapshot $shape "before_save"
        $doc.SaveAs($output, $PbFilePublication, $false)
    }
    finally {
        Close-Document $doc
        Close-PubPublisherApplication $app
    }

    $app2 = $null
    $doc2 = $null
    $after = $null
    try {
        $app2 = New-PubPublisherApplication
        $doc2 = $app2.Open($output, $true, $false)
        if ([int]$doc2.Pages.Item(1).Shapes.Count -ne 1) { throw "Expected one shape after reopen" }
        $after = Get-FontSnapshot $doc2.Pages.Item(1).Shapes.Item(1) "fresh_reopen"
    }
    finally {
        Close-Document $doc2
        Close-PubPublisherApplication $app2
    }

    $file = Get-Item -LiteralPath $output
    return [ordered]@{
        requested_stylistic_sets = $value
        before_save = $before
        fresh_reopen = $after
        output = [ordered]@{
            size = [int64]$file.Length
            sha256 = (Get-FileHash -LiteralPath $output -Algorithm SHA256).Hash.ToLowerInvariant()
            private_local = $true
        }
    }
}

$arms = @()
foreach ($value in @(0,1,2,3)) { $arms += Invoke-Arm $value }

$result = [ordered]@{
    schema = "pub-opentype-stylisticsets-2019-01/v1"
    experiment_id = $ExpectedExperiment
    requested_font = "Gabriola"
    arms = $arms
    verdict = if (@($arms | Where-Object { $_.fresh_reopen.stylistic_sets.state -ne "value" -or [int]$_.fresh_reopen.stylistic_sets.value -ne [int]$_.requested_stylistic_sets }).Count -eq 0) { "semantic-roundtrip-confirmed" } else { "inconclusive" }
    boundary = "This experiment proves COM/save/reopen semantics only. Exact Quill carrier localization is a separate analyzer step."
}
Write-PubJson -Value $result -Path (Join-Path $analysisDir "opentype-stylisticsets-2019-01.json")
@(
    "experiment=$ExpectedExperiment",
    "requested_font=Gabriola",
    "arms=0,1,2,3",
    "verdict=$($result.verdict)"
) | Set-Content -LiteralPath (Join-Path $logDir "opentype-stylisticsets-2019-01.txt") -Encoding ASCII
