param(
    [Parameter(Mandatory = $true)][string]$Input034,
    [Parameter(Mandatory = $true)][string]$Input053,
    [Parameter(Mandatory = $true)][string]$OutputRoot
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot "../..")).Path
Import-Module (Join-Path $RepoRoot "tools/windows/pub-runtime/PubRuntime.psm1") -Force

$Targets = @(
    [pscustomobject]@{
        id = "034"
        input = $Input034
        sha256 = "0d2278960a0101ced0a1ba6d5922f5a26ea37b76b3ab00ef21e21873359d4a5a"
        bytes = [int64]1019904
        page_index = 1
    },
    [pscustomobject]@{
        id = "053"
        input = $Input053
        sha256 = "70f21e6856945d091469fd79f815674128e1cbf7e160e352aa0e80968ae924f8"
        bytes = [int64]631296
        page_index = 1
    }
)

function Release-Com($Value) {
    if ($null -ne $Value -and [Runtime.InteropServices.Marshal]::IsComObject($Value)) {
        try { [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($Value) } catch {}
    }
}

function Read-Value {
    param([scriptblock]$Getter)
    try { return & $Getter } catch { return $null }
}

function Crop-Class {
    param($Value)
    if ($null -eq $Value) { return "unavailable" }
    $number = [double]$Value
    if ([math]::Abs($number) -lt 0.000001) { return "zero" }
    if ($number -gt 0) { return "positive_inward" }
    return "negative_outward"
}

function Rotation-Class {
    param($Value)
    if ($null -eq $Value) { return "unavailable" }
    $angle = [double]$Value
    while ($angle -lt 0) { $angle += 360.0 }
    while ($angle -ge 360.0) { $angle -= 360.0 }
    foreach ($cardinal in @(0.0, 90.0, 180.0, 270.0)) {
        if ([math]::Abs($angle - $cardinal) -lt 0.000001) {
            return ("cardinal_{0}" -f [int]$cardinal)
        }
    }
    return "noncardinal"
}

function Nodes-Class {
    param($Count)
    if ($null -eq $Count) { return "unavailable" }
    if ([int]$Count -eq 0) { return "zero" }
    return "nonzero"
}

function Has-NonzeroCrop {
    param($Crop)
    return @($Crop.left, $Crop.top, $Crop.right, $Crop.bottom) |
        Where-Object { $_ -notin @("zero", "unavailable") } |
        Select-Object -First 1
}

# Admit both exact sources before any Publisher COM activation.
$Admitted = @()
foreach ($target in $Targets) {
    $resolved = (Resolve-Path -LiteralPath $target.input).Path
    $item = Get-Item -LiteralPath $resolved
    if ([int64]$item.Length -ne [int64]$target.bytes) {
        throw "$($target.id) source size mismatch: expected $($target.bytes) got $($item.Length)"
    }
    $sha = (Get-FileHash -LiteralPath $resolved -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($sha -ne $target.sha256) {
        throw "$($target.id) source SHA mismatch: expected $($target.sha256) got $sha"
    }
    $Admitted += [pscustomobject]@{
        id = $target.id
        path = $resolved
        sha256 = $sha
        bytes = [int64]$item.Length
        page_index = [int]$target.page_index
    }
}

if (-not [IO.Path]::IsPathRooted($OutputRoot)) {
    $OutputRoot = Join-Path $RepoRoot $OutputRoot
}
if (Test-Path -LiteralPath $OutputRoot) {
    if (@(Get-ChildItem -LiteralPath $OutputRoot -Force).Count -ne 0) {
        throw "OutputRoot must be empty: $OutputRoot"
    }
} else {
    New-Item -ItemType Directory -Force -Path $OutputRoot | Out-Null
}

$publisher = Get-PubPublisherIdentity
if (-not $publisher.available) { throw "Publisher COM unavailable" }
if ($publisher.version.state -ne "value" -or [string]$publisher.version.value -ne "16.0") {
    throw "Publisher Version mismatch"
}
if ($publisher.build.state -ne "value" -or [string]$publisher.build.value -ne "12527") {
    throw "Publisher Build mismatch"
}

$TargetReceipts = @()

foreach ($target in $Admitted) {
    $app = $null
    $doc = $null
    $page = $null
    $shapeFacts = @()

    try {
        $app = New-PubPublisherApplication
        $doc = $app.Open($target.path, $true, $false)
        if ([int]$doc.Pages.Count -lt $target.page_index) {
            throw "$($target.id) page $($target.page_index) missing in native Publisher"
        }
        $page = $doc.Pages.Item($target.page_index)

        for ($shapeIndex = 1; $shapeIndex -le [int]$page.Shapes.Count; $shapeIndex++) {
            $shape = $null
            $pictureFormat = $null
            $nodes = $null
            $adjustments = $null
            $groupItems = $null
            try {
                $shape = $page.Shapes.Item($shapeIndex)
                $left = Read-Value { [double]$shape.Left }
                $top = Read-Value { [double]$shape.Top }
                $width = Read-Value { [double]$shape.Width }
                $height = Read-Value { [double]$shape.Height }
                $zOrder = Read-Value { [int]$shape.ZOrderPosition }
                $shapeType = Read-Value { [int]$shape.Type }
                $autoShapeType = Read-Value { [int]$shape.AutoShapeType }
                $rotation = Read-Value { [double]$shape.Rotation }
                $shapeId = Read-Value { [int64]$shape.ID }

                $nodesCount = $null
                try {
                    $nodes = $shape.Nodes
                    if ($null -ne $nodes) { $nodesCount = [int]$nodes.Count }
                } catch {}

                $adjustmentsCount = $null
                try {
                    $adjustments = $shape.Adjustments
                    if ($null -ne $adjustments) { $adjustmentsCount = [int]$adjustments.Count }
                } catch {}

                $groupItemsCount = $null
                try {
                    $groupItems = $shape.GroupItems
                    if ($null -ne $groupItems) { $groupItemsCount = [int]$groupItems.Count }
                } catch {}

                $pictureAccessible = $false
                $crop = [ordered]@{
                    left = "unavailable"
                    top = "unavailable"
                    right = "unavailable"
                    bottom = "unavailable"
                }
                $pictureMetadata = $null
                try {
                    $pictureFormat = $shape.PictureFormat
                    if ($null -ne $pictureFormat) {
                        $pictureAccessible = $true
                        $crop = [ordered]@{
                            left = Crop-Class (Read-Value { [double]$pictureFormat.CropLeft })
                            top = Crop-Class (Read-Value { [double]$pictureFormat.CropTop })
                            right = Crop-Class (Read-Value { [double]$pictureFormat.CropRight })
                            bottom = Crop-Class (Read-Value { [double]$pictureFormat.CropBottom })
                        }
                        $pictureMetadata = [ordered]@{
                            image_format = Read-Value { [int]$pictureFormat.ImageFormat }
                            is_empty = Read-Value { [bool]$pictureFormat.IsEmpty }
                            is_linked = Read-Value { [bool]$pictureFormat.IsLinked }
                            has_alpha_channel = Read-Value { [bool]$pictureFormat.HasAlphaChannel }
                            has_transparency_color = Read-Value { [bool]$pictureFormat.HasTransparencyColor }
                            horizontal_picture_locking = Read-Value { [int]$pictureFormat.HorizontalPictureLocking }
                            vertical_picture_locking = Read-Value { [int]$pictureFormat.VerticalPictureLocking }
                        }
                    }
                } catch {}

                $shapeFacts += [pscustomobject]@{
                    shape_index = $shapeIndex
                    shape_id = $shapeId
                    shape_type = $shapeType
                    auto_shape_type = $autoShapeType
                    rotation_class = Rotation-Class $rotation
                    z_order_position = $zOrder
                    left = $left
                    top = $top
                    width = $width
                    height = $height
                    nodes_count = $nodesCount
                    adjustments_count = $adjustmentsCount
                    group_items_count = $groupItemsCount
                    picture_accessible = $pictureAccessible
                    crop = $crop
                    picture_metadata = $pictureMetadata
                }
            } finally {
                Release-Com $groupItems
                Release-Com $adjustments
                Release-Com $nodes
                Release-Com $pictureFormat
                Release-Com $shape
            }
        }
    } finally {
        Release-Com $page
        if ($null -ne $doc) {
            try { $doc.Close() } catch {}
            Release-Com $doc
        }
        Close-PubPublisherApplication $app
    }

    $cropped = @()
    foreach ($picture in @($shapeFacts | Where-Object {
        $_.picture_accessible -and (Has-NonzeroCrop $_.crop)
    })) {
        $overlapCounts = @{}
        if ($null -ne $picture.left -and $null -ne $picture.top -and
            $null -ne $picture.width -and $null -ne $picture.height) {
            $pictureRight = [double]$picture.left + [double]$picture.width
            $pictureBottom = [double]$picture.top + [double]$picture.height
            foreach ($other in $shapeFacts) {
                if ($other.shape_index -eq $picture.shape_index) { continue }
                if ($null -eq $other.left -or $null -eq $other.top -or
                    $null -eq $other.width -or $null -eq $other.height) { continue }
                $otherRight = [double]$other.left + [double]$other.width
                $otherBottom = [double]$other.top + [double]$other.height
                $overlaps = ([double]$picture.left -lt $otherRight) -and
                    ([double]$other.left -lt $pictureRight) -and
                    ([double]$picture.top -lt $otherBottom) -and
                    ([double]$other.top -lt $pictureBottom)
                if (-not $overlaps) { continue }

                $relation = if ($null -ne $picture.z_order_position -and $null -ne $other.z_order_position) {
                    if ([int]$other.z_order_position -lt [int]$picture.z_order_position) {
                        "earlier"
                    } elseif ([int]$other.z_order_position -gt [int]$picture.z_order_position) {
                        "later"
                    } else {
                        "same"
                    }
                } elseif ($other.shape_index -lt $picture.shape_index) {
                    "earlier_collection"
                } else {
                    "later_collection"
                }
                $key = "{0}|type:{1}|auto:{2}|nodes:{3}" -f
                    $relation,
                    $(if ($null -eq $other.shape_type) { "unavailable" } else { [string]$other.shape_type }),
                    $(if ($null -eq $other.auto_shape_type) { "unavailable" } else { [string]$other.auto_shape_type }),
                    (Nodes-Class $other.nodes_count)
                if (-not $overlapCounts.ContainsKey($key)) { $overlapCounts[$key] = 0 }
                $overlapCounts[$key] += 1
            }
        }

        $cropped += [ordered]@{
            shape_index = [int]$picture.shape_index
            shape_id = $picture.shape_id
            shape_type = $picture.shape_type
            auto_shape_type = $picture.auto_shape_type
            rotation_class = $picture.rotation_class
            native_geometry = [ordered]@{
                nodes_count = $picture.nodes_count
                adjustments_count = $picture.adjustments_count
                group_items_count = $picture.group_items_count
            }
            crop_edge_classes = $picture.crop
            picture_metadata = $picture.picture_metadata
            overlap_class_counts = [ordered]@{} + $overlapCounts
        }
    }

    $after = Get-Item -LiteralPath $target.path
    $afterSha = (Get-FileHash -LiteralPath $target.path -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($afterSha -ne $target.sha256 -or [int64]$after.Length -ne [int64]$target.bytes) {
        throw "$($target.id) source changed during read-only native probe"
    }

    $TargetReceipts += [ordered]@{
        id = $target.id
        source_sha256 = $target.sha256
        source_bytes = [int64]$target.bytes
        source_unchanged_after_probe = $true
        page_index = [int]$target.page_index
        page_shape_count = $shapeFacts.Count
        picture_format_shape_count = @($shapeFacts | Where-Object { $_.picture_accessible }).Count
        cropped_picture_count = $cropped.Count
        cropped_pictures = $cropped
    }
}

$receipt = [ordered]@{
    schema = "chaptera.publisher-crop-native-034-053.v1"
    publisher = [ordered]@{
        version = $publisher.version
        build = $publisher.build
        name = $publisher.name
    }
    targets = $TargetReceipts
    claims = [ordered]@{
        opened_read_only = $true
        save_invoked = $false
        print_invoked = $false
        export_invoked = $false
        macro_execution_invoked = $false
        text_emitted = $false
        coordinates_emitted = $false
        raw_crop_values_emitted = $false
        pub_bytes_emitted = $false
    }
}

$receiptPath = Join-Path $OutputRoot "publisher-crop-native-034-053.json"
Write-PubJson -Value $receipt -Path $receiptPath

$repoSha = (& git -C $RepoRoot rev-parse HEAD).Trim()
if ($LASTEXITCODE -ne 0 -or $repoSha -notmatch '^[0-9a-f]{40}$') {
    throw "Unable to bind repository HEAD for native return"
}
$probeBlob = (& git -C $RepoRoot hash-object -- "tools/windows/run_publisher_crop_native_034_053_direct.ps1").Trim()
if ($LASTEXITCODE -ne 0 -or $probeBlob -notmatch '^[0-9a-f]{40}$') {
    throw "Unable to bind native probe blob"
}

$metadata = [ordered]@{
    schema = "chaptera.publisher-crop-native-034-053-return.v1"
    repo_sha = $repoSha
    probe_git_blob = $probeBlob
    receipt_file = "publisher-crop-native-034-053.json"
    sources = @(
        [ordered]@{ id = "034"; sha256 = $Targets[0].sha256; bytes = $Targets[0].bytes },
        [ordered]@{ id = "053"; sha256 = $Targets[1].sha256; bytes = $Targets[1].bytes }
    )
    claims = [ordered]@{
        source_bytes_in_return = $false
        exact_source_identity_bound_before_publisher = $true
    }
}
$metadataPath = Join-Path $OutputRoot "publisher-crop-native-034-053-return.json"
Write-PubJson -Value $metadata -Path $metadataPath

$zipPath = Join-Path $OutputRoot "publisher-crop-native-034-053-return.zip"
Compress-Archive -LiteralPath @($receiptPath, $metadataPath) -DestinationPath $zipPath -CompressionLevel Optimal

Write-Host "PUBLISHER_CROP_NATIVE_034_053_RETURN=$zipPath"
Get-Content -LiteralPath $receiptPath -Raw
