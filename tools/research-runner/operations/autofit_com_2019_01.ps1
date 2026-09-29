param(
    [Parameter(Mandatory = $true)][string]$PacketPath,
    [Parameter(Mandatory = $true)][string]$OutputRoot
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$ExpectedExperiment = "AUTOFIT-COM-2019-01"
$PbFilePublication = 1
$MsoTextOrientationHorizontal = 1

Import-Module (Join-Path (Split-Path -Parent (Split-Path -Parent $PSScriptRoot)) "windows\pub-runtime\PubRuntime.psm1") -Force

$packet = Get-Content -LiteralPath $PacketPath -Raw | ConvertFrom-Json
if ([string]$packet.id -ne $ExpectedExperiment) { throw "Unexpected experiment id: $($packet.id)" }
if ([string]::IsNullOrWhiteSpace([string]$env:PUB_RESEARCH_FIXTURE)) { throw "PUB_RESEARCH_FIXTURE missing" }

$analysisDir = Join-Path $OutputRoot "analysis"
$logDir = Join-Path $OutputRoot "logs"
$privateDir = Join-Path $OutputRoot "private\autofit-com-2019-01"
New-Item -ItemType Directory -Force -Path $analysisDir,$logDir,$privateDir | Out-Null

function Close-Document($doc) {
    if ($null -eq $doc) { return }
    try { $doc.Saved = $true } catch {}
    try { $doc.Close() } catch {}
    try { [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($doc) } catch {}
}

function Get-TextFrameSnapshot($shape, [string]$phase) {
    return [ordered]@{
        phase = $phase
        shape_id = Get-PubSafeValue { [int]$shape.ID } "Shape.ID"
        left = Get-PubSafeValue { [double]$shape.Left } "Shape.Left"
        top = Get-PubSafeValue { [double]$shape.Top } "Shape.Top"
        width = Get-PubSafeValue { [double]$shape.Width } "Shape.Width"
        height = Get-PubSafeValue { [double]$shape.Height } "Shape.Height"
        auto_fit_text = Get-PubSafeValue { [int]$shape.TextFrame.AutoFitText } "TextFrame.AutoFitText"
        overflowing = Get-PubSafeValue { [bool]$shape.TextFrame.Overflowing } "TextFrame.Overflowing"
        font_name = Get-PubSafeValue { [string]$shape.TextFrame.TextRange.Font.Name } "Font.Name"
        font_size = Get-PubSafeValue { [double]$shape.TextFrame.TextRange.Font.Size } "Font.Size"
        text_length = Get-PubSafeValue { [int]$shape.TextFrame.TextRange.Length } "TextRange.Length"
    }
}

function Invoke-Arm([string]$name, [int]$mode) {
    $armDir = Join-Path $privateDir $name
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
        $shape = $doc.Pages.Item(1).Shapes.AddTextbox($MsoTextOrientationHorizontal, 72, 72, 140, 42)
        $shape.TextFrame.TextRange.Text = "Chaptera Publisher AutoFit discriminator with enough text to overflow a deliberately small frame."
        $shape.TextFrame.TextRange.Font.Name = "Arial"
        $shape.TextFrame.TextRange.Font.Size = 18
        $shape.TextFrame.AutoFitText = $mode
        $before = Get-TextFrameSnapshot $shape "before_save"
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
        $after = Get-TextFrameSnapshot $doc2.Pages.Item(1).Shapes.Item(1) "fresh_reopen"
    }
    finally {
        Close-Document $doc2
        Close-PubPublisherApplication $app2
    }

    $file = Get-Item -LiteralPath $output
    return [ordered]@{
        arm = $name
        requested_mode = $mode
        before_save = $before
        fresh_reopen = $after
        output = [ordered]@{
            size = [int64]$file.Length
            sha256 = (Get-FileHash -LiteralPath $output -Algorithm SHA256).Hash.ToLowerInvariant()
            private_local = $true
        }
    }
}

$specs = @(
    [ordered]@{ name = "none"; mode = 0 },
    [ordered]@{ name = "shrink"; mode = 1 },
    [ordered]@{ name = "bestfit"; mode = 2 }
)

$arms = @()
foreach ($spec in $specs) { $arms += Invoke-Arm -name $spec.name -mode $spec.mode }

$roundtripOk = $true
foreach ($arm in $arms) {
    if ($arm.fresh_reopen.auto_fit_text.state -ne "value" -or [int]$arm.fresh_reopen.auto_fit_text.value -ne [int]$arm.requested_mode) {
        $roundtripOk = $false
    }
}

$result = [ordered]@{
    schema = "pub-autofit-com-2019-01/v1"
    experiment_id = $ExpectedExperiment
    arms = $arms
    verdict = if ($roundtripOk) { "semantic-roundtrip-confirmed" } else { "inconclusive" }
    boundary = "This experiment covers COM None/ShrinkOnOverflow/BestFit only. UI Grow textbox to fit and exact persisted carrier localization are separate discriminators."
}
Write-PubJson -Value $result -Path (Join-Path $analysisDir "autofit-com-2019-01.json")
@(
    "experiment=$ExpectedExperiment",
    "arms=none:0,shrink:1,bestfit:2",
    "verdict=$($result.verdict)"
) | Set-Content -LiteralPath (Join-Path $logDir "autofit-com-2019-01.txt") -Encoding ASCII
