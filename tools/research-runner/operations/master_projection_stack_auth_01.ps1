param(
    [Parameter(Mandatory = $true)][string]$InputPath,
    [Parameter(Mandatory = $true)][string]$OutputRoot
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$ExpectedSha256 = "0ca858ed4806e81da2964d75d54d25a2ac0c6126074e9f82ea33b87701de4ade"
$ExperimentId = "MASTER-PROJECTION-STACK-AUTH-01"
$Schema = "chaptera.master-projection-stack-auth.v1"

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "../../..")).Path
Import-Module (Join-Path $repoRoot "tools/windows/pub-runtime/PubRuntime.psm1") -Force

$analysisDir = Join-Path $OutputRoot "analysis"
$privateDir = Join-Path $OutputRoot "private"
$logDir = Join-Path $OutputRoot "logs"
New-Item -ItemType Directory -Force -Path $analysisDir,$privateDir,$logDir | Out-Null

$resultPath = Join-Path $analysisDir "master-projection-stack-auth-01.json"
$logPath = Join-Path $logDir "master-projection-stack-auth-01.txt"
$environmentPath = Join-Path $OutputRoot "environment.json"

function Release-Com($Value) {
    if ($null -ne $Value -and [Runtime.InteropServices.Marshal]::IsComObject($Value)) {
        try { [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($Value) } catch {}
    }
}

function Color-DistanceSq {
    param(
        [int]$R, [int]$G, [int]$B,
        [int]$TargetR, [int]$TargetG, [int]$TargetB
    )
    $dr = $R - $TargetR
    $dg = $G - $TargetG
    $db = $B - $TargetB
    return ($dr * $dr) + ($dg * $dg) + ($db * $db)
}

function Read-OverlapPixel {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [double]$PageWidth,
        [double]$PageHeight,
        [double]$Left,
        [double]$Top,
        [double]$Width,
        [double]$Height
    )
    Add-Type -AssemblyName System.Drawing
    $bitmap = $null
    try {
        $bitmap = [System.Drawing.Bitmap]::FromFile($Path)
        if ($bitmap.Width -le 0 -or $bitmap.Height -le 0) {
            throw "Saved page picture has invalid dimensions."
        }
        $centerX = $Left + ($Width / 2.0)
        $centerY = $Top + ($Height / 2.0)
        $x = [int][Math]::Floor(($centerX / $PageWidth) * $bitmap.Width)
        $y = [int][Math]::Floor(($centerY / $PageHeight) * $bitmap.Height)
        $x = [Math]::Max(0, [Math]::Min($bitmap.Width - 1, $x))
        $y = [Math]::Max(0, [Math]::Min($bitmap.Height - 1, $y))
        $pixel = $bitmap.GetPixel($x, $y)
        return [ordered]@{
            image_width = [int]$bitmap.Width
            image_height = [int]$bitmap.Height
            sample_x = $x
            sample_y = $y
            r = [int]$pixel.R
            g = [int]$pixel.G
            b = [int]$pixel.B
            a = [int]$pixel.A
        }
    }
    finally {
        if ($null -ne $bitmap) { $bitmap.Dispose() }
    }
}

function Add-Test-Rectangle {
    param(
        [Parameter(Mandatory = $true)]$Shapes,
        [double]$Left,
        [double]$Top,
        [double]$Width,
        [double]$Height,
        [int]$Rgb,
        [string]$Name
    )
    # Office msoShapeRectangle = 1.
    $shape = $Shapes.AddShape(1, $Left, $Top, $Width, $Height)
    try {
        try { $shape.Name = $Name } catch {}
        $shape.Fill.Solid()
        $shape.Fill.ForeColor.RGB = $Rgb
        $shape.Line.Visible = 0
        return [ordered]@{
            name = $Name
            z_order_position = [int]$shape.ZOrderPosition
            left = [double]$shape.Left
            top = [double]$shape.Top
            width = [double]$shape.Width
            height = [double]$shape.Height
        }
    }
    finally {
        Release-Com $shape
    }
}

function Invoke-Arm {
    param(
        [Parameter(Mandatory = $true)][string]$ArmId,
        [Parameter(Mandatory = $true)][ValidateSet("master_first","page_first")][string]$CreationOrder,
        [Parameter(Mandatory = $true)][string]$SourcePath
    )

    $armRoot = Join-Path $privateDir $ArmId
    New-Item -ItemType Directory -Force -Path $armRoot | Out-Null
    $armPub = Join-Path $armRoot "$ArmId.pub"
    $pngPath = Join-Path $armRoot "$ArmId.png"
    Copy-Item -LiteralPath $SourcePath -Destination $armPub -Force

    # COLORREF layout used by Office RGB properties: 0x00BBGGRR.
    $masterRgb = 0x000000FF # red: R=255,G=0,B=0
    $pageRgb = 0x00FF0000   # blue: R=0,G=0,B=255

    $left = 144.0
    $top = 144.0
    $width = 180.0
    $height = 180.0

    $app = $null
    $doc = $null
    $page = $null
    $master = $null
    $masterShapes = $null
    $pageShapes = $null
    $before = Get-PubFileRecord -Path $armPub
    $created = [ordered]@{}
    $pageWidth = $null
    $pageHeight = $null
    $pageId = $null
    $masterPageId = $null
    $ignoreMaster = $null
    $countsBefore = $null

    try {
        $app = New-PubPublisherApplication
        $doc = $app.Open($armPub, $false, $false)
        if ([int]$doc.Pages.Count -ne 1) {
            throw "Fixture must have exactly one publication page for ordinary single-master stacking authority."
        }
        $page = $doc.Pages.Item(1)
        $master = $page.Master
        $pageWidth = [double]$page.Width
        $pageHeight = [double]$page.Height
        $pageId = [long]$page.PageID
        $masterPageId = [long]$master.PageID
        $ignoreMaster = [bool]$page.IgnoreMaster
        $masterIsTwoPage = [bool]$master.IsTwoPageMaster
        if ($masterIsTwoPage) {
            throw "Fixture uses a Two Page Master; outside ordinary single-master stacking scope."
        }
        if ($ignoreMaster) {
            throw "Fixture page 1 ignores its master; unsuitable for stacking authority."
        }
        $masterShapes = $master.Shapes
        $pageShapes = $page.Shapes
        $countsBefore = [ordered]@{
            master_shapes = [int]$masterShapes.Count
            page_shapes = [int]$pageShapes.Count
        }

        if ($CreationOrder -eq "master_first") {
            $created.master = Add-Test-Rectangle -Shapes $masterShapes -Left $left -Top $top -Width $width -Height $height -Rgb $masterRgb -Name "CHAPTERA_STACK_MASTER"
            $created.page = Add-Test-Rectangle -Shapes $pageShapes -Left $left -Top $top -Width $width -Height $height -Rgb $pageRgb -Name "CHAPTERA_STACK_PAGE"
        }
        else {
            $created.page = Add-Test-Rectangle -Shapes $pageShapes -Left $left -Top $top -Width $width -Height $height -Rgb $pageRgb -Name "CHAPTERA_STACK_PAGE"
            $created.master = Add-Test-Rectangle -Shapes $masterShapes -Left $left -Top $top -Width $width -Height $height -Rgb $masterRgb -Name "CHAPTERA_STACK_MASTER"
        }

        $doc.Save()
    }
    finally {
        Release-Com $pageShapes
        Release-Com $masterShapes
        Release-Com $master
        Release-Com $page
        if ($null -ne $doc) {
            try { $doc.Close() } catch {}
            Release-Com $doc
        }
        Close-PubPublisherApplication $app
    }

    $saved = Get-PubFileRecord -Path $armPub
    if ([string]$saved.sha256 -eq [string]$before.sha256) {
        throw "Disposable arm did not persist any mutation."
    }

    $reopen = [ordered]@{}
    $app = $null
    $doc = $null
    $page = $null
    $master = $null
    $masterShapes = $null
    $pageShapes = $null
    try {
        $app = New-PubPublisherApplication
        $doc = $app.Open($armPub, $true, $false)
        $page = $doc.Pages.Item(1)
        $master = $page.Master
        $masterShapes = $master.Shapes
        $pageShapes = $page.Shapes

        $reopen.page_id = [long]$page.PageID
        $reopen.master_page_id = [long]$master.PageID
        $reopen.ignore_master = [bool]$page.IgnoreMaster
        $reopen.master_is_two_page_master = [bool]$master.IsTwoPageMaster
        $reopen.master_shapes = [int]$masterShapes.Count
        $reopen.page_shapes = [int]$pageShapes.Count
        $reopen.page_width = [double]$page.Width
        $reopen.page_height = [double]$page.Height

        if ($reopen.page_id -ne $pageId -or $reopen.master_page_id -ne $masterPageId) {
            throw "Page/master identity changed across Save/reopen."
        }
        if ($reopen.ignore_master) {
            throw "Page unexpectedly ignored master after Save/reopen."
        }
        if ($reopen.master_is_two_page_master) {
            throw "Master became or remained a Two Page Master after Save/reopen."
        }
        if ($reopen.master_shapes -ne ($countsBefore.master_shapes + 1)) {
            throw "Master shape count did not persist exactly one added shape."
        }
        if ($reopen.page_shapes -ne ($countsBefore.page_shapes + 1)) {
            throw "Page-local shape count did not persist exactly one added shape."
        }

        $page.SaveAsPicture($pngPath)
    }
    finally {
        Release-Com $pageShapes
        Release-Com $masterShapes
        Release-Com $master
        Release-Com $page
        if ($null -ne $doc) {
            try { $doc.Close() } catch {}
            Release-Com $doc
        }
        Close-PubPublisherApplication $app
    }

    if (-not (Test-Path -LiteralPath $pngPath -PathType Leaf)) {
        throw "Publisher Page.SaveAsPicture did not create PNG."
    }

    $pixel = Read-OverlapPixel -Path $pngPath -PageWidth $pageWidth -PageHeight $pageHeight -Left $left -Top $top -Width $width -Height $height
    $distanceMaster = Color-DistanceSq -R $pixel.r -G $pixel.g -B $pixel.b -TargetR 255 -TargetG 0 -TargetB 0
    $distancePage = Color-DistanceSq -R $pixel.r -G $pixel.g -B $pixel.b -TargetR 0 -TargetG 0 -TargetB 255
    $winner = if ($distanceMaster -lt $distancePage) { "master" } elseif ($distancePage -lt $distanceMaster) { "page_local" } else { "ambiguous" }

    return [ordered]@{
        arm_id = $ArmId
        creation_order = $CreationOrder
        source_copy_before = [ordered]@{
            sha256 = [string]$before.sha256
            size = [int64]$before.size
        }
        saved_pub = [ordered]@{
            sha256 = [string]$saved.sha256
            size = [int64]$saved.size
        }
        page_identity = [ordered]@{
            page_id = $pageId
            master_page_id = $masterPageId
            ignore_master = $ignoreMaster
            master_is_two_page_master = $masterIsTwoPage
        }
        counts_before = $countsBefore
        created_shapes = $created
        reopened = $reopen
        overlap = [ordered]@{
            master_expected_rgb = [ordered]@{ r = 255; g = 0; b = 0 }
            page_local_expected_rgb = [ordered]@{ r = 0; g = 0; b = 255 }
            sampled_pixel = $pixel
            master_distance_sq = $distanceMaster
            page_local_distance_sq = $distancePage
            winner = $winner
        }
        page_picture = [ordered]@{
            sha256 = (Get-FileHash -LiteralPath $pngPath -Algorithm SHA256).Hash.ToLowerInvariant()
            bytes = (Get-Item -LiteralPath $pngPath).Length
            retained_private_local = $true
        }
    }
}


function Invoke-Master-Visibility-Control {
    param(
        [Parameter(Mandatory = $true)][string]$SourcePath
    )

    $controlRoot = Join-Path $privateDir "master-visibility-control"
    New-Item -ItemType Directory -Force -Path $controlRoot | Out-Null
    $controlPub = Join-Path $controlRoot "master-visibility-control.pub"
    $pngPath = Join-Path $controlRoot "master-visibility-control.png"
    Copy-Item -LiteralPath $SourcePath -Destination $controlPub -Force

    $masterRgb = 0x000000FF
    $left = 144.0
    $top = 144.0
    $width = 180.0
    $height = 180.0

    $before = Get-PubFileRecord -Path $controlPub
    $app = $null
    $doc = $null
    $page = $null
    $master = $null
    $pageShapes = $null
    $masterShapes = $null
    $pageWidth = $null
    $pageHeight = $null
    $pageId = $null
    $masterPageId = $null
    $masterShapesBefore = $null
    $removedPageShapeCount = 0

    try {
        $app = New-PubPublisherApplication
        $doc = $app.Open($controlPub, $false, $false)
        if ([int]$doc.Pages.Count -ne 1) {
            throw "Visibility control requires exactly one publication page."
        }
        $page = $doc.Pages.Item(1)
        $master = $page.Master
        if ([bool]$page.IgnoreMaster) {
            throw "Visibility control page ignores its master."
        }
        if ([bool]$master.IsTwoPageMaster) {
            throw "Visibility control fixture uses a Two Page Master."
        }

        $pageWidth = [double]$page.Width
        $pageHeight = [double]$page.Height
        $pageId = [long]$page.PageID
        $masterPageId = [long]$master.PageID
        $pageShapes = $page.Shapes
        $masterShapes = $master.Shapes
        $masterShapesBefore = [int]$masterShapes.Count

        while ([int]$pageShapes.Count -gt 0) {
            $shape = $null
            try {
                $shape = $pageShapes.Item(1)
                $shape.Delete()
                $removedPageShapeCount += 1
            }
            finally {
                Release-Com $shape
            }
        }
        if ([int]$pageShapes.Count -ne 0) {
            throw "Visibility control could not remove all page-local shapes."
        }

        [void](Add-Test-Rectangle -Shapes $masterShapes -Left $left -Top $top -Width $width -Height $height -Rgb $masterRgb -Name "CHAPTERA_STACK_MASTER_VISIBILITY")
        $doc.Save()
    }
    finally {
        Release-Com $pageShapes
        Release-Com $masterShapes
        Release-Com $master
        Release-Com $page
        if ($null -ne $doc) {
            try { $doc.Close() } catch {}
            Release-Com $doc
        }
        Close-PubPublisherApplication $app
    }

    $saved = Get-PubFileRecord -Path $controlPub
    if ([string]$saved.sha256 -eq [string]$before.sha256) {
        throw "Master visibility control did not persist its disposable mutation."
    }

    $app = $null
    $doc = $null
    $page = $null
    $master = $null
    $pageShapes = $null
    $masterShapes = $null
    try {
        $app = New-PubPublisherApplication
        $doc = $app.Open($controlPub, $true, $false)
        $page = $doc.Pages.Item(1)
        $master = $page.Master
        $pageShapes = $page.Shapes
        $masterShapes = $master.Shapes

        if ([long]$page.PageID -ne $pageId -or [long]$master.PageID -ne $masterPageId) {
            throw "Visibility control page/master identity changed across Save/reopen."
        }
        if ([bool]$page.IgnoreMaster) {
            throw "Visibility control page unexpectedly ignores master after reopen."
        }
        if ([bool]$master.IsTwoPageMaster) {
            throw "Visibility control master is Two Page Master after reopen."
        }
        if ([int]$pageShapes.Count -ne 0) {
            throw "Visibility control page-local shapes survived deletion."
        }
        if ([int]$masterShapes.Count -ne ($masterShapesBefore + 1)) {
            throw "Visibility control master shape did not persist exactly once."
        }

        $page.SaveAsPicture($pngPath)
    }
    finally {
        Release-Com $pageShapes
        Release-Com $masterShapes
        Release-Com $master
        Release-Com $page
        if ($null -ne $doc) {
            try { $doc.Close() } catch {}
            Release-Com $doc
        }
        Close-PubPublisherApplication $app
    }

    if (-not (Test-Path -LiteralPath $pngPath -PathType Leaf)) {
        throw "Master visibility control did not create PNG."
    }

    $pixel = Read-OverlapPixel -Path $pngPath -PageWidth $pageWidth -PageHeight $pageHeight -Left $left -Top $top -Width $width -Height $height
    $distanceMaster = Color-DistanceSq -R $pixel.r -G $pixel.g -B $pixel.b -TargetR 255 -TargetG 0 -TargetB 0
    $distanceWhite = Color-DistanceSq -R $pixel.r -G $pixel.g -B $pixel.b -TargetR 255 -TargetG 255 -TargetB 255
    # Opaque center pixel should be effectively the authored red, not merely "closer to red than white".
    $masterVisible = $distanceMaster -le (3 * 16 * 16)

    return [ordered]@{
        removed_page_shape_count = $removedPageShapeCount
        master_shape_count_before = $masterShapesBefore
        sampled_pixel = $pixel
        master_distance_sq = $distanceMaster
        white_distance_sq = $distanceWhite
        master_visible = $masterVisible
        page_picture = [ordered]@{
            sha256 = (Get-FileHash -LiteralPath $pngPath -Algorithm SHA256).Hash.ToLowerInvariant()
            bytes = (Get-Item -LiteralPath $pngPath).Length
            retained_private_local = $true
        }
    }
}

$resolvedInput = (Resolve-Path -LiteralPath $InputPath).Path
$sourceBefore = Get-PubFileRecord -Path $resolvedInput
if ([string]$sourceBefore.sha256 -ne $ExpectedSha256) {
    throw "Exact source SHA mismatch: expected $ExpectedSha256 got $($sourceBefore.sha256)"
}

$environment = Get-PubEnvironmentManifest -SnapshotId $ExperimentId -RequirePublisher
Write-PubJson -Value $environment -Path $environmentPath

$masterVisibilityControl = Invoke-Master-Visibility-Control -SourcePath $resolvedInput

$arms = @(
    Invoke-Arm -ArmId "master-first" -CreationOrder "master_first" -SourcePath $resolvedInput
    Invoke-Arm -ArmId "page-first" -CreationOrder "page_first" -SourcePath $resolvedInput
)

$sourceAfter = Get-PubFileRecord -Path $resolvedInput
if ([string]$sourceAfter.sha256 -ne [string]$sourceBefore.sha256 -or [int64]$sourceAfter.size -ne [int64]$sourceBefore.size) {
    throw "Original exact source changed during stacking experiment."
}

$winners = @($arms | ForEach-Object { $_.overlap.winner } | Select-Object -Unique)
$verdict = if (-not [bool]$masterVisibilityControl.master_visible) {
    "save_as_picture_master_visibility_not_proven"
}
elseif ($winners.Count -eq 1 -and $winners[0] -eq "page_local") {
    "page_local_above_master"
}
elseif ($winners.Count -eq 1 -and $winners[0] -eq "master") {
    "master_above_page_local"
}
else {
    "ambiguous_or_creation_order_sensitive"
}

$result = [ordered]@{
    schema = $Schema
    experiment_id = $ExperimentId
    source = [ordered]@{
        sha256 = [string]$sourceBefore.sha256
        size = [int64]$sourceBefore.size
        unchanged_after_experiment = $true
    }
    publisher = [ordered]@{
        version = $environment.publisher.version
        build = $environment.publisher.build
        name = $environment.publisher.name
    }
    master_visibility_control = $masterVisibilityControl
    arms = $arms
    verdict = $verdict
    claims = [ordered]@{
        ordinary_single_master_scope_only = $true
        page_master_relation_mutated = $false
        source_original_mutated = $false
        generated_pub_uploaded = $false
        generated_page_picture_uploaded = $false
        save_close_fresh_reopen_per_arm = $true
        save_as_picture_master_visibility_proven = [bool]$masterVisibilityControl.master_visible
        two_page_master_excluded = $true
        cross_lane_order_decided_by_rendered_overlap = $true
        within_collection_zorder_is_not_cross_lane_authority = $true
        two_page_master_claimed = $false
        facing_page_claimed = $false
        ignore_master_persistence_claimed = $false
    }
}
Write-PubJson -Value $result -Path $resultPath

@(
    "experiment=$ExperimentId",
    "source_sha256=$($sourceBefore.sha256)",
    "master_visibility=$($masterVisibilityControl.master_visible)",
    "arm_master_first=$($arms[0].overlap.winner)",
    "arm_page_first=$($arms[1].overlap.winner)",
    "verdict=$verdict",
    "source_unchanged=true"
) | Set-Content -LiteralPath $logPath -Encoding ASCII
