param(
    [Parameter(Mandatory = $true)][string]$PacketPath,
    [Parameter(Mandatory = $true)][string]$OutputRoot
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
Import-Module (Join-Path $repo 'tools/windows/pub-runtime/PubRuntime.psm1') -Force
$packet = Get-Content -LiteralPath $PacketPath -Raw | ConvertFrom-Json
if ([string]$packet.id -ne 'AUTH-WRAP-01') { throw 'Unexpected research packet' }
$family = [string]$env:PUB_RESEARCH_AUTH_WRAP_FAMILY
if ($family -notin @('A', 'B')) { throw 'AUTH-WRAP family A or B is required by trusted direct launcher' }
$expectedSource = '6a825ba26ba35d6e885acdc62e859591ed37cb0ff7480b554b9cb362b644dfcf'
$fixture = [string]$env:PUB_RESEARCH_FIXTURE
$oracleExe = [string]$env:PUB_RESEARCH_AUTH_WRAP_ORACLE_EXE
if (-not (Test-Path -LiteralPath $fixture -PathType Leaf)) { throw 'Pinned fixture unavailable' }
if (-not (Test-Path -LiteralPath $oracleExe -PathType Leaf)) { throw 'Current-main AUTH-WRAP oracle tool unavailable' }
if (-not (Test-Path -LiteralPath (Join-Path $OutputRoot 'analysis/reset-receipt.json') -PathType Leaf)) {
    throw 'Verified real native reset receipt missing'
}
function Hash([string]$path) { (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant() }
if ((Hash $fixture) -ne $expectedSource -or [string]$packet.fixture.sha256 -ne $expectedSource) {
    throw 'AUTH-WRAP source SHA does not match registered SampleNewsletter fixture'
}
$privacyRoot = Join-Path $OutputRoot "private/auth-wrap-1188-$family"
$analysis = Join-Path $OutputRoot 'analysis'
$logs = Join-Path $OutputRoot 'logs'
New-Item -ItemType Directory -Force -Path $privacyRoot,$analysis,$logs | Out-Null
$base = Join-Path $privacyRoot 'base.pub'
$prepared = Join-Path $privacyRoot 'prepared.pub'
$saved = Join-Path $privacyRoot 'saved.pub'
$baseJson = Join-Path $privacyRoot 'base.inspect.json'
$preparedJson = Join-Path $privacyRoot 'prepared.inspect.json'
$savedJson = Join-Path $privacyRoot 'saved.inspect.json'
$comparisonJson = Join-Path $privacyRoot 'diff.json'
Copy-Item -LiteralPath $fixture -Destination $base
if ((Hash $base) -ne $expectedSource) { throw 'Private source copy drift' }

function Oracle([string[]]$argv) {
    & $oracleExe @argv
    if ($LASTEXITCODE -ne 0) { throw "AUTH-WRAP oracle failed with exit code $LASTEXITCODE" }
}
Oracle @('inspect',$base,$baseJson)
$initial = Get-Content -LiteralPath $baseJson -Raw | ConvertFrom-Json
$selected = @($initial.wrap_frames | Where-Object { @($_.refs).Count -gt 0 -and @($_.replacement_candidates).Count -gt 0 } | Sort-Object frame_seq_num)
if ($selected.Count -lt 1) { throw 'Exact fixture lacks a bounded 0x47 replacement candidate' }
# Select the single deterministic first candidate. The family B COM mutation is
# associated with the *referenced* obstacle, not an unrelated page shape.
$frame = $selected[0]
$frameSeq = [int]$frame.frame_seq_num
$obstacleSeq = [int]$frame.refs[0].target_seq_num
$rawObstacle = @($initial.shapes | Where-Object { [int]$_.seq_num -eq $obstacleSeq -and [string]$_.page_id -eq [string]$frame.page_id })
if ($rawObstacle.Count -ne 1) { throw 'Ambiguous source obstacle identity' }
$rawObstacle = $rawObstacle[0]
if ($family -eq 'A') {
    $replacementSeq = [int]$frame.replacement_candidates[0]
    Oracle @('patch-ref',$base,$prepared,[string]$frameSeq,'0',[string]$replacementSeq)
} else {
    Copy-Item -LiteralPath $base -Destination $prepared
}
Oracle @('inspect',$prepared,$preparedJson)

function ReleaseCom($object) {
    if ($null -ne $object -and [Runtime.InteropServices.Marshal]::IsComObject($object)) {
        try { [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($object) } catch {}
    }
}
function CloseDoc($document) {
    if ($null -eq $document) { return }
    try { $document.Saved = $true } catch {}
    try { $document.Close() } catch {}
    ReleaseCom $document
}
function FindShapeSlot($document, $sourceShape) {
    $matches = @()
    for ($pageIdx = 1; $pageIdx -le [int]$document.Pages.Count; $pageIdx++) {
        $page = $document.Pages.Item($pageIdx)
        try {
            if ([string]$page.PageID -ne [string]$sourceShape.page_id) { continue }
            for ($idx = 1; $idx -le [int]$page.Shapes.Count; $idx++) {
                $shape = $page.Shapes.Item($idx)
                try {
                    $coords = @(
                        [double]$shape.Left,
                        [double]$shape.Top,
                        [double]$shape.Width,
                        [double]$shape.Height
                    )
                    $raw = @(
                        [double]$sourceShape.bounds_emu.x,
                        [double]$sourceShape.bounds_emu.y,
                        [double]$sourceShape.bounds_emu.width,
                        [double]$sourceShape.bounds_emu.height
                    )
                    $same = $true
                    for ($j = 0; $j -lt 4; $j++) {
                        if ([Math]::Abs($coords[$j] * 12700.0 - $raw[$j]) -gt 635.0) { $same = $false }
                    }
                    if ($same) { $matches += [pscustomobject]@{ page = $pageIdx; shape = $idx } }
                } finally { ReleaseCom $shape }
            }
        } finally { ReleaseCom $page }
    }
    if ($matches.Count -ne 1) { throw "0x47 referenced COM obstacle has $($matches.Count) geometry matches" }
    return $matches[0]
}
function WrapState($shape) {
    $wrap = $shape.TextWrap
    try {
        return [ordered]@{
            type = [int]$wrap.Type
            side = [int]$wrap.Side
            distance_auto = [int]$wrap.DistanceAuto
            distance_left = [double]$wrap.DistanceLeft
            geometry = @([double]$shape.Left,[double]$shape.Top,[double]$shape.Width,[double]$shape.Height)
        }
    } finally { ReleaseCom $wrap }
}
function ExactGeometry($before, $after) {
    for ($i = 0; $i -lt 4; $i++) {
        if ([Math]::Abs([double]$before.geometry[$i] - [double]$after.geometry[$i]) -gt 0.0001) { return $false }
    }
    return $true
}

$preparedShaBeforeOpen = Hash $prepared
$app = $null
$doc = $null
$beforeCom = $null
$afterCom = $null
$changedCom = $false
$mutatedDistance = $null
try {
    $app = New-PubPublisherApplication
    $doc = $app.Open($prepared,$false,$false)
    $slot = FindShapeSlot -document $doc -sourceShape $rawObstacle
    $shape = $doc.Pages.Item([int]$slot.page).Shapes.Item([int]$slot.shape)
    try {
        $beforeCom = WrapState $shape
        if ($family -eq 'B') {
            # Exactly one documented writable property: DistanceLeft, and ONLY
            # on a shape that already exposes manual distance settings. Never
            # change Type, Side, DistanceAuto or geometry speculatively.
            if ([int]$beforeCom.distance_auto -ne 0) { throw 'DistanceAuto is enabled; no safe one-property mutation' }
            $wrap = $shape.TextWrap
            try {
                $newDistance = [double]$beforeCom.distance_left + 1.0
                $wrap.DistanceLeft = $newDistance
            } finally { ReleaseCom $wrap }
            $afterCom = WrapState $shape
            $mutatedDistance = $newDistance
            $changedCom = ([Math]::Abs([double]$afterCom.distance_left - $newDistance) -lt 0.0001 -and
                [int]$afterCom.type -eq [int]$beforeCom.type -and
                [int]$afterCom.side -eq [int]$beforeCom.side -and
                [int]$afterCom.distance_auto -eq [int]$beforeCom.distance_auto -and
                (ExactGeometry $beforeCom $afterCom))
            if (-not $changedCom) { throw 'COM changed more than one property or rejected requested wrap distance' }
        }
    } finally { ReleaseCom $shape }
    if ((Hash $prepared) -ne $preparedShaBeforeOpen) {
        throw 'Working PUB bytes changed in memory before SaveAs'
    }
    $doc.SaveAs($saved, 1, $false)
    if (-not (Test-Path -LiteralPath $saved -PathType Leaf)) { throw 'Publisher SaveAs did not produce candidate' }
} finally {
    CloseDoc $doc
    Close-PubPublisherApplication $app
}
if ((Hash $fixture) -ne $expectedSource) { throw 'Original registered PUB changed during native arm' }

# Prove a fresh Open after SaveAs, not simply an in-memory COM getter.
$app = $null
$doc = $null
$reopenedCom = $null
try {
    $app = New-PubPublisherApplication
    $doc = $app.Open($saved,$true,$false)
    $slot = FindShapeSlot -document $doc -sourceShape $rawObstacle
    $shape = $doc.Pages.Item([int]$slot.page).Shapes.Item([int]$slot.shape)
    try { $reopenedCom = WrapState $shape } finally { ReleaseCom $shape }
} finally {
    CloseDoc $doc
    Close-PubPublisherApplication $app
}
Oracle @('inspect',$saved,$savedJson)
& python (Join-Path $repo 'tools/research-runner/analysis/auth_wrap_diff_1188.py') `
    --family $family --frame-seq $frameSeq --before $baseJson --prepared $preparedJson `
    --saved $savedJson --output $comparisonJson
if ($LASTEXITCODE -ne 0) { throw 'AUTH-WRAP guarded raw comparison failed' }
$diff = Get-Content -LiteralPath $comparisonJson -Raw | ConvertFrom-Json
if ($family -eq 'B') {
    if (-not $changedCom) { throw 'Family B COM mutation absent' }
    if ([Math]::Abs([double]$reopenedCom.distance_left - [double]$mutatedDistance) -ge 0.0001) {
        throw 'Changed wrap distance did not persist after fresh Publisher reopen'
    }
    if (-not (ExactGeometry $beforeCom $reopenedCom)) { throw 'COM obstacle geometry moved unexpectedly' }
}
# No paragraph-range or PDF comparison has yet been admitted: explicitly
# mark layout as NOT OBSERVED. This is NOT a Format Law or Editor authority.
$capture = [ordered]@{
    executed = $true
    publisher_open_succeeded = $true
    pre_save_layout_captured = $false
    pre_save_com_wrap_captured = $true
    save_succeeded = $true
    reopen_succeeded = $true
    post_save_graph_captured = $true
    post_save_fopt_captured = $true
}
$report = [ordered]@{
    schema = 'chaptera.auth-wrap-1188.arm.v1'
    family = $family
    fixture_sha256 = $expectedSource
    publisher_exe_sha256 = (Hash $env:PUB_RESEARCH_PUBLISHER_EXE)
    baseline_count = @($initial.wrap_frames).Count
    capture = $capture
    diff = $diff
    one_wrap_property_changed_via_com = $changedCom
    # Fresh COM readback proves a property persisted, not which FOPT raw field
    # owns it. Do not claim a wire-level format relation from this alone.
    post_save_fopt_outcome = 'not_observed'
    layout_outcome = 'not_observed'
}
Write-PubJson -Value $report -Path (Join-Path $analysis 'auth-wrap-1188.json')
@("AUTH-WRAP-01 family=$family",'source_sha_verified=true','separate_pub_clone=true',
  'native_save_fresh_reopen=true','layout_authority=not_observed','format_law=not_accepted') |
    Set-Content -LiteralPath (Join-Path $logs 'auth-wrap-1188.txt') -Encoding ASCII
