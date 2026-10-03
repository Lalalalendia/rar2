param(
    [Parameter(Mandatory = $true)][string]$PacketPath,
    [Parameter(Mandatory = $true)][string]$OutputRoot
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$ExpectedExperiment = "LASTFMT-MIRROR-MODERN-01"
$ExpectedSourceSha256 = "1e7f38b3ce1d0d956815992b15d361c405fc4bbdced5cdabb3c4581327cc183e"
$TargetShapeId = 356
$TargetSpid = 1062
$ExpectedWrapEmu = 36576
$ExpectedRecolor = 134217731
$ExpectedSourceRevision = 19
$WrapPropertyIds = @(900, 901, 902, 903)

$packet = Get-Content -LiteralPath $PacketPath -Raw | ConvertFrom-Json
if ([string]$packet.id -ne $ExpectedExperiment) {
    throw "Unexpected experiment id: $($packet.id)"
}

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "../../..")).Path
Import-Module (Join-Path $repoRoot "tools/windows/pub-runtime/PubRuntime.psm1") -Force

$analysisDir = Join-Path $OutputRoot "analysis"
$logDir = Join-Path $OutputRoot "logs"
$privateDir = Join-Path $OutputRoot "private/lastfmt-mirror-modern-01"
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

function Get-FileRecord {
    param([Parameter(Mandatory = $true)][string]$Path)
    $resolved = (Resolve-Path -LiteralPath $Path).Path
    $item = Get-Item -LiteralPath $resolved
    return [ordered]@{
        path = $resolved
        sha256 = (Get-FileHash -LiteralPath $resolved -Algorithm SHA256).Hash.ToLowerInvariant()
        size = [long]$item.Length
    }
}

function Resolve-ExactFixture {
    $fixture = [string]$env:PUB_RESEARCH_FIXTURE
    if ([string]::IsNullOrWhiteSpace($fixture) -or -not (Test-Path -LiteralPath $fixture -PathType Leaf)) {
        throw "Set PUB_RESEARCH_FIXTURE to exact Publisher11 help.pub."
    }
    $record = Get-FileRecord -Path $fixture
    if ([string]$record.sha256 -ne $ExpectedSourceSha256) {
        throw "Pinned help.pub SHA-256 mismatch: expected $ExpectedSourceSha256, got $($record.sha256)"
    }
    return $record
}

function Build-ObserverTools {
    $targetDir = Join-Path $privateDir "cargo-target"
    $manifestPath = Join-Path $repoRoot "vendor/producer-a/Cargo.toml"
    $workspaceRoot = Split-Path -Parent $manifestPath
    $lockPath = Join-Path $workspaceRoot "Cargo.lock"
    New-Item -ItemType Directory -Force -Path $targetDir | Out-Null

    $cargoVersion = (& cargo --version 2>&1 | Out-String).Trim()
    if ($LASTEXITCODE -ne 0) {
        throw "cargo --version failed with exit code $LASTEXITCODE"
    }

    $lockExisted = Test-Path -LiteralPath $lockPath -PathType Leaf
    $generatedLock = $false
    try {
        if (-not $lockExisted) {
            Push-Location $repoRoot
            try {
                & cargo generate-lockfile --offline --manifest-path $manifestPath
                if ($LASTEXITCODE -ne 0) {
                    throw "offline Cargo.lock generation failed with exit code $LASTEXITCODE"
                }
            }
            finally {
                Pop-Location
            }
            if (-not (Test-Path -LiteralPath $lockPath -PathType Leaf)) {
                throw "Cargo.lock missing after offline generation"
            }
            $generatedLock = $true
        }

        $lockSha = (Get-FileHash -LiteralPath $lockPath -Algorithm SHA256).Hash.ToLowerInvariant()

        Push-Location $repoRoot
        try {
            & cargo build --locked --offline --release --target-dir $targetDir --manifest-path $manifestPath -p pub-reader --bin object_tracking_wrap_observer --bin escher_wrap_fopt_observer
            if ($LASTEXITCODE -ne 0) {
                throw "LASTFMT observer tools offline build failed with exit code $LASTEXITCODE"
            }
        }
        finally {
            Pop-Location
        }

        $tracking = Join-Path $targetDir "release/object_tracking_wrap_observer.exe"
        $escher = Join-Path $targetDir "release/escher_wrap_fopt_observer.exe"
        foreach ($tool in @($tracking,$escher)) {
            if (-not (Test-Path -LiteralPath $tool -PathType Leaf)) {
                throw "Observer helper missing after offline build: $tool"
            }
        }

        return [pscustomobject]@{
            tracking = $tracking
            tracking_sha256 = (Get-FileHash -LiteralPath $tracking -Algorithm SHA256).Hash.ToLowerInvariant()
            escher = $escher
            escher_sha256 = (Get-FileHash -LiteralPath $escher -Algorithm SHA256).Hash.ToLowerInvariant()
            cargo_version = $cargoVersion
            cargo_lock_sha256 = $lockSha
            cargo_lock_origin = $(if ($generatedLock) { "generated-offline-for-run" } else { "preexisting" })
        }
    }
    finally {
        if ($generatedLock -and (Test-Path -LiteralPath $lockPath -PathType Leaf)) {
            Remove-Item -LiteralPath $lockPath -Force
        }
    }
}

function Invoke-Tracking {
    param(
        [Parameter(Mandatory = $true)][string]$Tool,
        [Parameter(Mandatory = $true)][string]$Source,
        [Parameter(Mandatory = $true)][long]$TargetSeq,
        [Parameter(Mandatory = $true)][string]$Output
    )
    & $Tool $Source $TargetSeq $Output
    if ($LASTEXITCODE -ne 0) {
        throw "object_tracking_wrap_observer failed with exit code $LASTEXITCODE"
    }
    return Get-Content -LiteralPath $Output -Raw | ConvertFrom-Json
}

function Invoke-Escher {
    param(
        [Parameter(Mandatory = $true)][string]$Tool,
        [Parameter(Mandatory = $true)][string]$Source,
        [Parameter(Mandatory = $true)][string]$Output
    )
    & $Tool $Source $Output
    if ($LASTEXITCODE -ne 0) {
        throw "escher_wrap_fopt_observer failed with exit code $LASTEXITCODE"
    }
    return Get-Content -LiteralPath $Output -Raw | ConvertFrom-Json
}

function Get-ExactEscherTarget {
    param(
        [Parameter(Mandatory = $true)]$Receipt,
        [Parameter(Mandatory = $true)][long]$PublisherShapeId
    )
    $matches = @($Receipt.observations | Where-Object { $null -ne $_.publisher_shape_id -and [long]$_.publisher_shape_id -eq $PublisherShapeId })
    if ($matches.Count -eq 1) { return $matches[0] }
    return $null
}

function Get-SpidTarget {
    param(
        [Parameter(Mandatory = $true)]$Receipt,
        [Parameter(Mandatory = $true)][long]$Spid
    )
    $matches = @($Receipt.observations | Where-Object { $null -ne $_.officeart_spid -and [long]$_.officeart_spid -eq $Spid })
    if ($matches.Count -eq 1) { return $matches[0] }
    return $null
}

function Test-ExactWrapFopts {
    param([Parameter(Mandatory = $true)]$Target)
    foreach ($propertyId in $WrapPropertyIds) {
        $matches = @($Target.wrap_properties | Where-Object { [int]$_.property_id -eq $propertyId })
        if ($matches.Count -ne 1) { return $false }
        if ([long]$matches[0].op -ne $ExpectedWrapEmu) { return $false }
    }
    return $true
}

function Test-ExactRecolorFopt {
    param([Parameter(Mandatory = $true)]$Target)
    $matches = @($Target.recolor_properties)
    return ($matches.Count -eq 1 -and [int]$matches[0].property_id -eq 0x011A -and [long]$matches[0].op -eq $ExpectedRecolor)
}

function Get-WrapFoptSummary {
    param($Target)
    if ($null -eq $Target) { return $null }
    $summary = [ordered]@{}
    foreach ($propertyId in $WrapPropertyIds) {
        $matches = @($Target.wrap_properties | Where-Object { [int]$_.property_id -eq $propertyId })
        $summary["fopt_$propertyId"] = @($matches | ForEach-Object {
            [ordered]@{
                opid = [int]$_.opid
                op = [long]$_.op
            }
        })
    }
    return [ordered]@{
        publisher_shape_id = $Target.publisher_shape_id
        officeart_spid = $Target.officeart_spid
        officeart_shape_type = $Target.officeart_shape_type
        properties = $summary
        recolor = @($Target.recolor_properties | ForEach-Object {
            [ordered]@{
                property_id = [int]$_.property_id
                opid = [int]$_.opid
                op = [long]$_.op
            }
        })
        exact_expected_wrap = [bool](Test-ExactWrapFopts -Target $Target)
        exact_expected_recolor = [bool](Test-ExactRecolorFopt -Target $Target)
    }
}

function Get-TrackingSummary {
    param([Parameter(Mandatory = $true)]$Receipt)
    $obs = @($Receipt.observer.observations)
    $entry = if ($obs.Count -eq 1) { $obs[0] } else { $null }
    $ecpRecolorScalars = @()
    if ($null -ne $entry -and $null -ne $entry.PSObject.Properties["ecp_recolor_scalars"]) {
        $ecpRecolorScalars = @($entry.ecp_recolor_scalars)
    }
    return [ordered]@{
        serialization_revision = [int]$Receipt.observer.serialization_revision
        tracking_object_count = [int]$Receipt.observer.tracking_object_count
        target_observation_count = $obs.Count
        target_oh_track = [long]$Receipt.target_oh_track
        dx_wrap_dist_left = if ($null -ne $entry -and $null -ne $entry.dx_wrap_dist_left) { [long]$entry.dx_wrap_dist_left.value } else { $null }
        dy_wrap_dist_top = if ($null -ne $entry -and $null -ne $entry.dy_wrap_dist_top) { [long]$entry.dy_wrap_dist_top.value } else { $null }
        dx_wrap_dist_right = if ($null -ne $entry -and $null -ne $entry.dx_wrap_dist_right) { [long]$entry.dx_wrap_dist_right.value } else { $null }
        dy_wrap_dist_bottom = if ($null -ne $entry -and $null -ne $entry.dy_wrap_dist_bottom) { [long]$entry.dy_wrap_dist_bottom.value } else { $null }
        ecp_recolor_scalars = $ecpRecolorScalars
    }
}

function Test-ExactTrackingWrap {
    param([Parameter(Mandatory = $true)]$Summary)
    return (
        [int]$Summary.target_observation_count -eq 1 -and
        $null -ne $Summary.dx_wrap_dist_left -and [long]$Summary.dx_wrap_dist_left -eq $ExpectedWrapEmu -and
        $null -ne $Summary.dy_wrap_dist_top -and [long]$Summary.dy_wrap_dist_top -eq $ExpectedWrapEmu -and
        $null -ne $Summary.dx_wrap_dist_right -and [long]$Summary.dx_wrap_dist_right -eq $ExpectedWrapEmu -and
        $null -ne $Summary.dy_wrap_dist_bottom -and [long]$Summary.dy_wrap_dist_bottom -eq $ExpectedWrapEmu
    )
}

function Test-ExactTrackingMirror {
    param([Parameter(Mandatory = $true)]$Summary)
    if (-not (Test-ExactTrackingWrap -Summary $Summary)) { return $false }
    $values = @($Summary.ecp_recolor_scalars)
    return ($values.Count -eq 1 -and [long]$values[0].value -eq $ExpectedRecolor)
}

function Get-ComInventory {
    param([Parameter(Mandatory = $true)]$Document)
    $rows = @()
    for ($pageIndex = 1; $pageIndex -le [int]$Document.Pages.Count; $pageIndex++) {
        $page = $null
        try {
            $page = $Document.Pages.Item($pageIndex)
            for ($shapeIndex = 1; $shapeIndex -le [int]$page.Shapes.Count; $shapeIndex++) {
                $shape = $null
                try {
                    $shape = $page.Shapes.Item($shapeIndex)
                    $rows += [ordered]@{
                        page_index = $pageIndex
                        shape_index = $shapeIndex
                        shape_id = [long]$shape.ID
                    }
                }
                finally {
                    Release-Com $shape
                }
            }
        }
        finally {
            Release-Com $page
        }
    }
    return @($rows)
}

$source = Resolve-ExactFixture
$tools = Build-ObserverTools

$sourceTrackingPath = Join-Path $privateDir "source-tracking.json"
$sourceEscherPath = Join-Path $privateDir "source-escher.json"
$sourceTracking = Invoke-Tracking -Tool $tools.tracking -Source $source.path -TargetSeq $TargetShapeId -Output $sourceTrackingPath
$sourceTrackingSummary = Get-TrackingSummary -Receipt $sourceTracking
if ([int]$sourceTrackingSummary.serialization_revision -ne $ExpectedSourceRevision) {
    throw "Pinned help.pub serialization revision mismatch: expected $ExpectedSourceRevision, got $($sourceTrackingSummary.serialization_revision)"
}
if (-not (Test-ExactTrackingMirror -Summary $sourceTrackingSummary)) {
    throw "Pinned help.pub did not reproduce the exact source OplLastFmt GroupShape + EcpRecolor oracle for OhTrack=$TargetShapeId."
}

$sourceEscher = Invoke-Escher -Tool $tools.escher -Source $source.path -Output $sourceEscherPath
$sourceTarget = Get-ExactEscherTarget -Receipt $sourceEscher -PublisherShapeId $TargetShapeId
if ($null -eq $sourceTarget) {
    throw "Pinned help.pub did not expose unique Escher Publisher Shape.ID=$TargetShapeId."
}
if ([long]$sourceTarget.officeart_spid -ne $TargetSpid) {
    throw "Pinned help.pub target SPID mismatch: expected $TargetSpid, got $($sourceTarget.officeart_spid)"
}
if (-not (Test-ExactWrapFopts -Target $sourceTarget)) {
    throw "Pinned help.pub did not reproduce exact FOPT900..903=$ExpectedWrapEmu source oracle."
}
if (-not (Test-ExactRecolorFopt -Target $sourceTarget)) {
    throw "Pinned help.pub did not reproduce exact active Escher FOPT0x011A=$ExpectedRecolor recolor oracle."
}
$sourceFoptSummary = Get-WrapFoptSummary -Target $sourceTarget

$working = Join-Path $privateDir "working.pub"
Copy-Item -LiteralPath $source.path -Destination $working -Force
$preSave = Get-FileRecord -Path $working

$app = $null
$doc = $null
$publisherIdentity = $null
$beforeInventory = $null
$afterSaveInventory = $null
try {
    $app = New-PubPublisherApplication
    $publisherIdentity = [ordered]@{
        version = [string]$app.Version
        build = [string]$app.Build
    }
    $doc = $app.Open($working, $false, $false)
    $beforeInventory = Get-ComInventory -Document $doc
    $doc.Save()
    $afterSaveInventory = Get-ComInventory -Document $doc
}
finally {
    Close-Document $doc
    Close-PubPublisherApplication $app
}

$postSave = Get-FileRecord -Path $working

$app2 = $null
$doc2 = $null
$reopenInventory = $null
try {
    $app2 = New-PubPublisherApplication
    $doc2 = $app2.Open($working, $true, $false)
    $reopenInventory = Get-ComInventory -Document $doc2
}
finally {
    Close-Document $doc2
    Close-PubPublisherApplication $app2
}

$afterEscherPath = Join-Path $privateDir "after-escher.json"
$afterEscher = Invoke-Escher -Tool $tools.escher -Source $working -Output $afterEscherPath

$afterTarget = Get-ExactEscherTarget -Receipt $afterEscher -PublisherShapeId $TargetShapeId
$identityRoute = "same-publisher-shape-id"
if ($null -eq $afterTarget) {
    $afterTarget = Get-SpidTarget -Receipt $afterEscher -Spid $TargetSpid
    $identityRoute = if ($null -ne $afterTarget) { "same-officeart-spid" } else { "unresolved" }
}

$afterFoptSummary = Get-WrapFoptSummary -Target $afterTarget
$activeEscherPreserved = (
    $null -ne $afterTarget -and
    (Test-ExactWrapFopts -Target $afterTarget) -and
    (Test-ExactRecolorFopt -Target $afterTarget)
)

$resolvedAfterShapeId = if ($null -ne $afterTarget -and $null -ne $afterTarget.publisher_shape_id) {
    [long]$afterTarget.publisher_shape_id
} else {
    $null
}
$afterTrackingTarget = if ($null -ne $resolvedAfterShapeId) { $resolvedAfterShapeId } else { $TargetShapeId }
$afterTrackingPath = Join-Path $privateDir "after-tracking.json"
$afterTracking = Invoke-Tracking -Tool $tools.tracking -Source $working -TargetSeq $afterTrackingTarget -Output $afterTrackingPath
$afterTrackingSummary = Get-TrackingSummary -Receipt $afterTracking

$verdict = "semantic-loss"
if ($activeEscherPreserved) {
    if ($null -eq $afterTrackingSummary -or [int]$afterTrackingSummary.target_observation_count -eq 0) {
        $verdict = "mirror-retired"
    }
    elseif (Test-ExactTrackingMirror -Summary $afterTrackingSummary) {
        if ($identityRoute -eq "same-publisher-shape-id") {
            $verdict = "mirror-preserved"
        }
        else {
            $verdict = "mirror-rewritten-remapped"
        }
    }
    else {
        $verdict = "mirror-rewritten-remapped"
    }
}

$result = [ordered]@{
    schema = "chaptera.lastfmt-mirror-modern-01/v1"
    experiment_id = $ExpectedExperiment
    publisher = $publisherIdentity
    source = [ordered]@{
        sha256 = [string]$source.sha256
        size = [long]$source.size
        target_oh_track = $TargetShapeId
        target_spid = $TargetSpid
        expected_wrap_emu = $ExpectedWrapEmu
        expected_recolor = $ExpectedRecolor
        tracking = $sourceTrackingSummary
        escher = $sourceFoptSummary
    }
    native_lineage = [ordered]@{
        source_serialization_revision = [int]$sourceTrackingSummary.serialization_revision
        post_save_serialization_revision = [int]$afterTrackingSummary.serialization_revision
        save_count = 1
        save_operation = "Document.Save"
        semantic_mutation = $false
        save_as = $false
        fresh_read_only_reopen = $true
        pre_save_sha256 = [string]$preSave.sha256
        post_save_sha256 = [string]$postSave.sha256
        post_save_size = [long]$postSave.size
    }
    tooling = [ordered]@{
        object_tracking_helper_sha256 = [string]$tools.tracking_sha256
        escher_wrap_helper_sha256 = [string]$tools.escher_sha256
        cargo_version = [string]$tools.cargo_version
        cargo_lock_sha256 = [string]$tools.cargo_lock_sha256
        cargo_lock_origin = [string]$tools.cargo_lock_origin
    }
    identity = [ordered]@{
        route = $identityRoute
        source_publisher_shape_id = $TargetShapeId
        source_spid = $TargetSpid
        after_publisher_shape_id = $resolvedAfterShapeId
        after_spid = if ($null -ne $afterTarget -and $null -ne $afterTarget.officeart_spid) { [long]$afterTarget.officeart_spid } else { $null }
    }
    after = [ordered]@{
        active_escher_preserved = [bool]$activeEscherPreserved
        escher = $afterFoptSummary
        tracking = $afterTrackingSummary
    }
    com_inventory = [ordered]@{
        before_save = $beforeInventory
        after_save = $afterSaveInventory
        after_fresh_reopen = $reopenInventory
    }
    verdict = $verdict
    authority_boundary = "This receipt classifies one exact Publisher11 help.pub -> Publisher16/build12527 one-Document.Save lineage. It compares the already-proven OhTrack/Publisher-Shape.ID 356 OplLastFmt GroupShape wrap mirror and EcpRecolor payload against active Escher FOPT900..903 plus FOPT0x011A, following the same Publisher identity or, if needed, the same SPID. mirror-retired means the serialized legacy mirror is absent for the rejoined target while both tested active Escher semantics remain exact; it does not claim deletion of in-memory runtime state or universal retirement across all PUB families."
}

Write-PubJson -Value $result -Path (Join-Path $analysisDir "lastfmt-mirror-modern-01.json")
@(
    "experiment=$ExpectedExperiment",
    "source_sha256=$($result.source.sha256)",
    "post_save_sha256=$($result.native_lineage.post_save_sha256)",
    "source_serialization_revision=$($result.native_lineage.source_serialization_revision)",
    "post_save_serialization_revision=$($result.native_lineage.post_save_serialization_revision)",
    "publisher_version=$($result.publisher.version)",
    "publisher_build=$($result.publisher.build)",
    "identity_route=$($result.identity.route)",
    "after_publisher_shape_id=$($result.identity.after_publisher_shape_id)",
    "active_escher_preserved=$($result.after.active_escher_preserved)",
    "verdict=$($result.verdict)"
) | Set-Content -LiteralPath (Join-Path $logDir "lastfmt-mirror-modern-01.txt") -Encoding ASCII
