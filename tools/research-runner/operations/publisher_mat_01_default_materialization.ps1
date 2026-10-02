param(
    [Parameter(Mandatory = $true)][string]$PacketPath,
    [Parameter(Mandatory = $true)][string]$OutputRoot
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$ExpectedExperiment = "MAT-01-DEFAULT-MATERIALIZATION"
$EmuPerPoint = 12700.0

$packet = Get-Content -LiteralPath $PacketPath -Raw | ConvertFrom-Json
if ([string]$packet.id -ne $ExpectedExperiment) {
    throw "Unexpected experiment id: $($packet.id)"
}

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "../../..")).Path
Import-Module (Join-Path $repoRoot "tools/windows/pub-runtime/PubRuntime.psm1") -Force

$analysisDir = Join-Path $OutputRoot "analysis"
$logDir = Join-Path $OutputRoot "logs"
$privateDir = Join-Path $OutputRoot "private/mat-01-default-materialization"
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

function Resolve-BaseInputs {
    $fixture = [string]$env:PUB_RESEARCH_FIXTURE
    $receiptPath = [string]$env:PUB_RESEARCH_BASE_RECEIPT
    if ([string]::IsNullOrWhiteSpace($fixture) -or -not (Test-Path -LiteralPath $fixture -PathType Leaf)) {
        throw "Set PUB_RESEARCH_FIXTURE to the exact normalized PUB emitted by MODERN-BASE-PIN-01."
    }
    if ([string]::IsNullOrWhiteSpace($receiptPath) -or -not (Test-Path -LiteralPath $receiptPath -PathType Leaf)) {
        throw "Set PUB_RESEARCH_BASE_RECEIPT to analysis/modern-base-pin-01.json from the same T370 run."
    }

    $receipt = Get-Content -LiteralPath $receiptPath -Raw | ConvertFrom-Json
    if ([string]$receipt.schema -ne "chaptera.modern-base-pin.v1") {
        throw "Unexpected MODERN-BASE-PIN receipt schema."
    }
    $fixturePath = (Resolve-Path -LiteralPath $fixture).Path
    $fixtureSha = (Get-FileHash -LiteralPath $fixturePath -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($fixtureSha -ne [string]$receipt.native_lineage.post_save_sha256) {
        throw "Normalized base SHA does not match T370 receipt."
    }
    if (-not [bool]$receipt.native_lineage.fresh_read_only_reopen) {
        throw "T370 receipt lacks fresh read-only reopen."
    }
    if (-not [bool]$receipt.native_lineage.bounded_inventory_equal_after_reopen) {
        throw "T370 bounded COM inventory did not survive reopen."
    }

    return [pscustomobject]@{
        path = $fixturePath
        sha256 = $fixtureSha
        receipt = $receipt
    }
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

    $generatedLock = -not (Test-Path -LiteralPath $lockPath -PathType Leaf)
    try {
        if ($generatedLock) {
            Push-Location $repoRoot
            try {
                & cargo generate-lockfile --offline --manifest-path $manifestPath
                if ($LASTEXITCODE -ne 0) {
                    throw "MAT-01 offline Cargo.lock generation failed with exit code $LASTEXITCODE"
                }
            }
            finally {
                Pop-Location
            }
        }

        if (-not (Test-Path -LiteralPath $lockPath -PathType Leaf)) {
            throw "Cargo.lock missing before MAT-01 observer build"
        }
        $lockSha = (Get-FileHash -LiteralPath $lockPath -Algorithm SHA256).Hash.ToLowerInvariant()

        Push-Location $repoRoot
        try {
            & cargo build --locked --offline --release --target-dir $targetDir --manifest-path $manifestPath -p pub-reader --bin structural_base_manifest --bin object_tracking_wrap_observer
            if ($LASTEXITCODE -ne 0) {
                throw "MAT-01 observer tools offline build failed with exit code $LASTEXITCODE"
            }
        }
        finally {
            Pop-Location
        }

        $structural = Join-Path $targetDir "release/structural_base_manifest.exe"
        $tracking = Join-Path $targetDir "release/object_tracking_wrap_observer.exe"
        if (-not (Test-Path -LiteralPath $structural -PathType Leaf)) {
            throw "structural_base_manifest.exe missing after offline build"
        }
        if (-not (Test-Path -LiteralPath $tracking -PathType Leaf)) {
            throw "object_tracking_wrap_observer.exe missing after offline build"
        }

        return [pscustomobject]@{
            structural = $structural
            structural_sha256 = (Get-FileHash -LiteralPath $structural -Algorithm SHA256).Hash.ToLowerInvariant()
            tracking = $tracking
            tracking_sha256 = (Get-FileHash -LiteralPath $tracking -Algorithm SHA256).Hash.ToLowerInvariant()
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

function Invoke-Structural {
    param(
        [Parameter(Mandatory = $true)][string]$Tool,
        [Parameter(Mandatory = $true)][string]$Source,
        [Parameter(Mandatory = $true)][string]$Output
    )
    & $Tool $Source $Output
    if ($LASTEXITCODE -ne 0) {
        throw "structural_base_manifest failed with exit code $LASTEXITCODE"
    }
    return Get-Content -LiteralPath $Output -Raw | ConvertFrom-Json
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

function Find-ShapeLocationById {
    param(
        [Parameter(Mandatory = $true)]$Document,
        [Parameter(Mandatory = $true)][long]$ShapeId
    )
    $matches = @()
    for ($pageIndex = 1; $pageIndex -le [int]$Document.Pages.Count; $pageIndex++) {
        $page = $null
        try {
            $page = $Document.Pages.Item($pageIndex)
            for ($shapeIndex = 1; $shapeIndex -le [int]$page.Shapes.Count; $shapeIndex++) {
                $shape = $null
                try {
                    $shape = $page.Shapes.Item($shapeIndex)
                    if ([long]$shape.ID -eq $ShapeId) {
                        $matches += [pscustomobject]@{
                            page_index = $pageIndex
                            shape_index = $shapeIndex
                        }
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
    if ($matches.Count -eq 0) { return $null }
    if ($matches.Count -ne 1) {
        throw "Shape.ID=$ShapeId is not document-unique."
    }
    return $matches[0]
}

function Get-WrapSnapshot {
    param(
        [Parameter(Mandatory = $true)]$Shape,
        [Parameter(Mandatory = $true)][string]$Phase
    )
    $wrap = $null
    try {
        $wrap = $Shape.TextWrap
        return [ordered]@{
            phase = $Phase
            shape_id = [long]$Shape.ID
            wrap_type = [long]$wrap.Type
            distance_left_points = [double]$wrap.DistanceLeft
            distance_top_points = [double]$wrap.DistanceTop
            distance_right_points = [double]$wrap.DistanceRight
            distance_bottom_points = [double]$wrap.DistanceBottom
        }
    }
    finally {
        Release-Com $wrap
    }
}

function Select-MatTarget {
    param(
        [Parameter(Mandatory = $true)][string]$BasePath,
        [Parameter(Mandatory = $true)]$Structural,
        [Parameter(Mandatory = $true)]$Tools
    )

    $app = $null
    $doc = $null
    try {
        $app = New-PubPublisherApplication
        $doc = $app.Open($BasePath, $true, $false)

        foreach ($candidate in @($Structural.manifest.candidates | Sort-Object contents_seq_num)) {
            $seq = [long]$candidate.contents_seq_num
            $location = Find-ShapeLocationById -Document $doc -ShapeId $seq
            if ($null -eq $location) { continue }

            $page = $null
            $shape = $null
            $wrap = $null
            try {
                $page = $doc.Pages.Item([int]$location.page_index)
                $shape = $page.Shapes.Item([int]$location.shape_index)
                $wrap = $shape.TextWrap
                [void][double]$wrap.DistanceLeft
                [void][double]$wrap.DistanceTop
            }
            catch {
                continue
            }
            finally {
                Release-Com $wrap
                Release-Com $shape
                Release-Com $page
            }

            $trackingPath = Join-Path $privateDir ("select-tracking-{0}.json" -f $seq)
            $tracking = Invoke-Tracking -Tool $Tools.tracking -Source $BasePath -TargetSeq $seq -Output $trackingPath
            if (@($tracking.observer.observations).Count -ge 1) {
                return [pscustomobject]@{
                    contents_seq_num = $seq
                    page_index = [int]$location.page_index
                    shape_index = [int]$location.shape_index
                }
            }
        }
    }
    finally {
        Close-Document $doc
        Close-PubPublisherApplication $app
    }

    throw "No bounded ordinary structural candidate is both COM TextWrap-readable and represented in ObjectTracking."
}

function Get-Candidate {
    param(
        [Parameter(Mandatory = $true)]$Structural,
        [Parameter(Mandatory = $true)][long]$TargetSeq
    )
    $matches = @($Structural.manifest.candidates | Where-Object { [long]$_.contents_seq_num -eq $TargetSeq })
    if ($matches.Count -ne 1) {
        throw "Expected exactly one structural candidate for target seq $TargetSeq; found $($matches.Count)"
    }
    return $matches[0]
}

function Get-FoptSnapshot {
    param(
        [Parameter(Mandatory = $true)]$Candidate,
        [Parameter(Mandatory = $true)][int]$PropertyId
    )
    $matches = @(
        $Candidate.escher_shape.fopts |
            ForEach-Object { @($_.properties) } |
            Where-Object { (([int]$_.opid -band 0x3FFF) -eq $PropertyId) }
    )
    return [ordered]@{
        property_id = $PropertyId
        count = $matches.Count
        entries = @($matches | ForEach-Object {
            [ordered]@{
                opid = [int]$_.opid
                op = [long]$_.op
                source = $_.source
            }
        })
    }
}

function Get-TrackingSnapshot {
    param(
        [Parameter(Mandatory = $true)]$Tracking
    )
    $obs = @($Tracking.observer.observations)
    $entry = if ($obs.Count -eq 1) { $obs[0] } else { $null }
    return [ordered]@{
        observation_count = $obs.Count
        observations = $obs
        dx_wrap_dist_left = if ($null -ne $entry -and $null -ne $entry.dx_wrap_dist_left) { [long]$entry.dx_wrap_dist_left.value } else { $null }
        dy_wrap_dist_top = if ($null -ne $entry -and $null -ne $entry.dy_wrap_dist_top) { [long]$entry.dy_wrap_dist_top.value } else { $null }
        dx_wrap_dist_right = if ($null -ne $entry -and $null -ne $entry.dx_wrap_dist_right) { [long]$entry.dx_wrap_dist_right.value } else { $null }
        dy_wrap_dist_bottom = if ($null -ne $entry -and $null -ne $entry.dy_wrap_dist_bottom) { [long]$entry.dy_wrap_dist_bottom.value } else { $null }
    }
}

function Invoke-MatArm {
    param(
        [Parameter(Mandatory = $true)][string]$Axis,
        [Parameter(Mandatory = $true)][string]$Mode,
        [Parameter(Mandatory = $true)][string]$BasePath,
        [Parameter(Mandatory = $true)][long]$TargetSeq,
        [Parameter(Mandatory = $true)]$Tools
    )

    $name = "$Axis-$Mode"
    $armDir = Join-Path $privateDir $name
    New-Item -ItemType Directory -Force -Path $armDir | Out-Null
    $working = Join-Path $armDir "working.pub"
    Copy-Item -LiteralPath $BasePath -Destination $working -Force

    $app = $null
    $doc = $null
    $page = $null
    $shape = $null
    $wrap = $null
    $before = $null
    $requested = $null
    try {
        $app = New-PubPublisherApplication
        $doc = $app.Open($working, $false, $false)
        $location = Find-ShapeLocationById -Document $doc -ShapeId $TargetSeq
        if ($null -eq $location) { throw "Target Shape.ID=$TargetSeq not found in $name arm." }
        $page = $doc.Pages.Item([int]$location.page_index)
        $shape = $page.Shapes.Item([int]$location.shape_index)
        $before = Get-WrapSnapshot -Shape $shape -Phase "before"

        $wrap = $shape.TextWrap
        $current = if ($Axis -eq "left") { [double]$wrap.DistanceLeft } else { [double]$wrap.DistanceTop }
        if ($Mode -eq "equal") {
            $requested = $current
            if ($Axis -eq "left") { $wrap.DistanceLeft = [single]$requested } else { $wrap.DistanceTop = [single]$requested }
        }
        elseif ($Mode -eq "nondefault") {
            $requested = $current + 1.0
            if ($Axis -eq "left") { $wrap.DistanceLeft = [single]$requested } else { $wrap.DistanceTop = [single]$requested }
        }
        elseif ($Mode -ne "implicit") {
            throw "Unknown MAT-01 mode $Mode"
        }

        Release-Com $wrap
        $wrap = $null
        $doc.Save()
    }
    finally {
        Release-Com $wrap
        Release-Com $shape
        Release-Com $page
        Close-Document $doc
        Close-PubPublisherApplication $app
    }

    $app2 = $null
    $doc2 = $null
    $page2 = $null
    $shape2 = $null
    $after = $null
    try {
        $app2 = New-PubPublisherApplication
        $doc2 = $app2.Open($working, $true, $false)
        $location2 = Find-ShapeLocationById -Document $doc2 -ShapeId $TargetSeq
        if ($null -eq $location2) { throw "Target Shape.ID=$TargetSeq missing after reopen in $name arm." }
        $page2 = $doc2.Pages.Item([int]$location2.page_index)
        $shape2 = $page2.Shapes.Item([int]$location2.shape_index)
        $after = Get-WrapSnapshot -Shape $shape2 -Phase "fresh_reopen"
    }
    finally {
        Release-Com $shape2
        Release-Com $page2
        Close-Document $doc2
        Close-PubPublisherApplication $app2
    }

    $structuralPath = Join-Path $armDir "structural.json"
    $trackingPath = Join-Path $armDir "tracking.json"
    $structural = Invoke-Structural -Tool $Tools.structural -Source $working -Output $structuralPath
    $tracking = Invoke-Tracking -Tool $Tools.tracking -Source $working -TargetSeq $TargetSeq -Output $trackingPath
    $candidate = Get-Candidate -Structural $structural -TargetSeq $TargetSeq

    return [ordered]@{
        axis = $Axis
        mode = $Mode
        requested_points = $requested
        before = $before
        fresh_reopen = $after
        output_sha256 = (Get-FileHash -LiteralPath $working -Algorithm SHA256).Hash.ToLowerInvariant()
        contents_chunk = $candidate.contents_chunk
        fopt_900 = Get-FoptSnapshot -Candidate $candidate -PropertyId 900
        fopt_901 = Get-FoptSnapshot -Candidate $candidate -PropertyId 901
        tracking = Get-TrackingSnapshot -Tracking $tracking
    }
}

function Points-ToEmu([double]$Points) {
    return [long][math]::Round($Points * $EmuPerPoint)
}

function Evaluate-Axis {
    param(
        [Parameter(Mandatory = $true)][string]$Axis,
        [Parameter(Mandatory = $true)]$Implicit,
        [Parameter(Mandatory = $true)]$Equal,
        [Parameter(Mandatory = $true)]$Nondefault
    )
    if ($Axis -eq "left") {
        $implicitPoints = [double]$Implicit.fresh_reopen.distance_left_points
        $equalPoints = [double]$Equal.fresh_reopen.distance_left_points
        $nondefaultPoints = [double]$Nondefault.fresh_reopen.distance_left_points
        $implicitTracking = $Implicit.tracking.dx_wrap_dist_left
        $equalTracking = $Equal.tracking.dx_wrap_dist_left
        $nondefaultTracking = $Nondefault.tracking.dx_wrap_dist_left
        $implicitFopt = $Implicit.fopt_900
        $equalFopt = $Equal.fopt_900
        $nondefaultFopt = $Nondefault.fopt_900
    }
    else {
        $implicitPoints = [double]$Implicit.fresh_reopen.distance_top_points
        $equalPoints = [double]$Equal.fresh_reopen.distance_top_points
        $nondefaultPoints = [double]$Nondefault.fresh_reopen.distance_top_points
        $implicitTracking = $Implicit.tracking.dy_wrap_dist_top
        $equalTracking = $Equal.tracking.dy_wrap_dist_top
        $nondefaultTracking = $Nondefault.tracking.dy_wrap_dist_top
        $implicitFopt = $Implicit.fopt_901
        $equalFopt = $Equal.fopt_901
        $nondefaultFopt = $Nondefault.fopt_901
    }

    $implicitEmu = Points-ToEmu $implicitPoints
    $equalEmu = Points-ToEmu $equalPoints
    $nondefaultEmu = Points-ToEmu $nondefaultPoints
    $nondefaultFoptExact = (
        [int]$nondefaultFopt.count -eq 1 -and
        [long]$nondefaultFopt.entries[0].op -eq $nondefaultEmu
    )
    $materializedDefault = (
        $null -ne $implicitTracking -and
        [long]$implicitTracking -eq $implicitEmu -and
        [int]$implicitFopt.count -eq 0
    )
    $equalResolved = ($null -ne $equalTracking -and [long]$equalTracking -eq $equalEmu)
    $nondefaultResolved = (
        $null -ne $nondefaultTracking -and
        [long]$nondefaultTracking -eq $nondefaultEmu -and
        $nondefaultFoptExact
    )

    return [ordered]@{
        axis = $Axis
        implicit_effective_emu = $implicitEmu
        equal_effective_emu = $equalEmu
        nondefault_effective_emu = $nondefaultEmu
        implicit_tracking_emu = $implicitTracking
        equal_tracking_emu = $equalTracking
        nondefault_tracking_emu = $nondefaultTracking
        implicit_fopt_count = [int]$implicitFopt.count
        equal_fopt_count = [int]$equalFopt.count
        nondefault_fopt_count = [int]$nondefaultFopt.count
        materialized_default_with_sparse_fopt = [bool]$materializedDefault
        explicit_equal_resolves_same_effective_value = [bool]$equalResolved
        nondefault_resolved_and_fopt_exact = [bool]$nondefaultResolved
        pass = [bool]($materializedDefault -and $equalResolved -and $nondefaultResolved)
    }
}

$base = Resolve-BaseInputs
$tools = Build-ObserverTools

$baseStructuralPath = Join-Path $privateDir "base-structural.json"
$baseStructural = Invoke-Structural -Tool $tools.structural -Source $base.path -Output $baseStructuralPath
if ([string]$baseStructural.source_sha256 -ne $base.sha256) {
    throw "Base structural receipt source SHA mismatch."
}
$target = Select-MatTarget -BasePath $base.path -Structural $baseStructural -Tools $tools

$leftImplicit = Invoke-MatArm -Axis "left" -Mode "implicit" -BasePath $base.path -TargetSeq $target.contents_seq_num -Tools $tools
$leftEqual = Invoke-MatArm -Axis "left" -Mode "equal" -BasePath $base.path -TargetSeq $target.contents_seq_num -Tools $tools
$leftNondefault = Invoke-MatArm -Axis "left" -Mode "nondefault" -BasePath $base.path -TargetSeq $target.contents_seq_num -Tools $tools
$topImplicit = Invoke-MatArm -Axis "top" -Mode "implicit" -BasePath $base.path -TargetSeq $target.contents_seq_num -Tools $tools
$topEqual = Invoke-MatArm -Axis "top" -Mode "equal" -BasePath $base.path -TargetSeq $target.contents_seq_num -Tools $tools
$topNondefault = Invoke-MatArm -Axis "top" -Mode "nondefault" -BasePath $base.path -TargetSeq $target.contents_seq_num -Tools $tools

$leftVerdict = Evaluate-Axis -Axis "left" -Implicit $leftImplicit -Equal $leftEqual -Nondefault $leftNondefault
$topVerdict = Evaluate-Axis -Axis "top" -Implicit $topImplicit -Equal $topEqual -Nondefault $topNondefault
$overall = if ([bool]$leftVerdict.pass -and [bool]$topVerdict.pass) {
    "resolved-default-materialization-confirmed"
} else {
    "not-confirmed"
}

$result = [ordered]@{
    schema = "chaptera.mat-01-default-materialization/v1"
    experiment_id = $ExpectedExperiment
    base = [ordered]@{
        source_sha256 = $base.sha256
        t370_receipt_schema = [string]$base.receipt.schema
        t370_post_save_sha256 = [string]$base.receipt.native_lineage.post_save_sha256
    }
    tooling = [ordered]@{
        structural_base_helper_sha256 = [string]$tools.structural_sha256
        object_tracking_helper_sha256 = [string]$tools.tracking_sha256
        cargo_version = [string]$tools.cargo_version
        cargo_lock_sha256 = [string]$tools.cargo_lock_sha256
        cargo_lock_origin = [string]$tools.cargo_lock_origin
    }
    target = [ordered]@{
        contents_seq_num = [long]$target.contents_seq_num
        page_index = [int]$target.page_index
        shape_index = [int]$target.shape_index
    }
    arms = @(
        $leftImplicit,$leftEqual,$leftNondefault,
        $topImplicit,$topEqual,$topNondefault
    )
    left = $leftVerdict
    top = $topVerdict
    verdict = $overall
    authority_boundary = "MAT-01 proves only whether the named OplLastFmt GroupShape wrap-distance projection materializes the Publisher2019 effective TextWrap default while local OfficeArt FOPT remains sparse, repeated on independent left/top axes. It does not establish conflict precedence; AUTH-5A owns that later question."
}

Write-PubJson -Value $result -Path (Join-Path $analysisDir "mat-01-default-materialization.json")
@(
    "experiment=$ExpectedExperiment",
    "base_sha256=$($base.sha256)",
    "target_contents_seq=$($target.contents_seq_num)",
    "structural_base_helper_sha256=$($tools.structural_sha256)",
    "object_tracking_helper_sha256=$($tools.tracking_sha256)",
    "cargo_lock_sha256=$($tools.cargo_lock_sha256)",
    "cargo_lock_origin=$($tools.cargo_lock_origin)",
    "left_pass=$($leftVerdict.pass)",
    "top_pass=$($topVerdict.pass)",
    "verdict=$overall"
) | Set-Content -LiteralPath (Join-Path $logDir "mat-01-default-materialization.txt") -Encoding ASCII
