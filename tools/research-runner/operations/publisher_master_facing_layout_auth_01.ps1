param(
    [Parameter(Mandatory = $true)][string]$PacketPath,
    [Parameter(Mandatory = $true)][string]$OutputRoot
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$ExperimentId = "MASTER-FACING-LAYOUT-AUTH-01"
$ExpectedFixtureSha256 = "5bf6057b8b11c8ee4a421d93885ae6e9e7c03a7a42d541e6d0df33497c08c33b"
$PbFilePublication = 1
$MsoShapeRectangle = 1
$MsoTextOrientationHorizontal = 1
$TagName = "PUB_T825_ROLE"

$packet = Get-Content -LiteralPath $PacketPath -Raw | ConvertFrom-Json
if ([string]$packet.id -ne $ExperimentId) {
    throw "Unexpected experiment id: $($packet.id)"
}

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "../../..")).Path
Import-Module (Join-Path $repoRoot "tools/windows/pub-runtime/PubRuntime.psm1") -Force

$analysisDir = Join-Path $OutputRoot "analysis"
$logDir = Join-Path $OutputRoot "logs"
$privateDir = Join-Path $OutputRoot "private/master-facing-layout-auth-01"
New-Item -ItemType Directory -Force -Path $analysisDir,$logDir,$privateDir | Out-Null

function Release-Com($Value) {
    if ($null -ne $Value -and [Runtime.InteropServices.Marshal]::IsComObject($Value)) {
        try { [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($Value) } catch {}
    }
}

function Close-Document($Document) {
    if ($null -eq $Document) { return }
    try { $Document.Saved = $true } catch {}
    try { $Document.Close() } catch {}
    Release-Com $Document
}

function Get-Sha256([string]$Path) {
    return (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
}

function Get-RelativeOutputPath([string]$Path) {
    $full = [IO.Path]::GetFullPath($Path)
    $root = [IO.Path]::GetFullPath($OutputRoot).TrimEnd([char]'\',[char]'/') + [IO.Path]::DirectorySeparatorChar
    if ($full.StartsWith($root, [StringComparison]::OrdinalIgnoreCase)) {
        return $full.Substring($root.Length).Replace('\','/')
    }
    return [IO.Path]::GetFileName($full)
}

function Get-FileSummary([string]$Path) {
    $item = Get-Item -LiteralPath $Path
    return [ordered]@{
        relative_path = Get-RelativeOutputPath $Path
        size = [int64]$item.Length
        sha256 = Get-Sha256 $Path
    }
}

function Get-OptionalValue {
    param(
        [Parameter(Mandatory = $true)][scriptblock]$Getter
    )
    try {
        return [ordered]@{ state = "value"; value = (& $Getter) }
    }
    catch {
        return [ordered]@{
            state = "error"
            hresult = ("0x{0:X8}" -f ($_.Exception.HResult -band 0xffffffff))
            message = $_.Exception.Message
        }
    }
}

function Find-PublicationPageById {
    param($Document,[int]$PageId)
    for ($i = 1; $i -le [int]$Document.Pages.Count; $i++) {
        $page = $null
        try {
            $page = $Document.Pages.Item($i)
            if ([int]$page.PageID -eq $PageId) { return $page }
        }
        finally {
            if ($null -ne $page -and [int]$page.PageID -ne $PageId) { Release-Com $page }
        }
    }
    throw "Publication PageID=$PageId not found."
}

function Find-MasterPageById {
    param($Document,[int]$PageId)
    for ($i = 1; $i -le [int]$Document.MasterPages.Count; $i++) {
        $page = $null
        try {
            $page = $Document.MasterPages.Item($i)
            if ([int]$page.PageID -eq $PageId) { return $page }
        }
        finally {
            if ($null -ne $page -and [int]$page.PageID -ne $PageId) { Release-Com $page }
        }
    }
    throw "Master PageID=$PageId not found."
}

function Get-TaggedShapeSnapshots {
    param($Page)
    $rows = @()
    for ($i = 1; $i -le [int]$Page.Shapes.Count; $i++) {
        $shape = $null
        try {
            $shape = $Page.Shapes.Item($i)
            $role = $null
            for ($j = 1; $j -le [int]$shape.Tags.Count; $j++) {
                $tag = $null
                try {
                    $tag = $shape.Tags.Item($j)
                    if ([string]$tag.Name -eq $TagName) { $role = [string]$tag.Value }
                }
                finally { Release-Com $tag }
            }
            if ($null -ne $role) {
                $text = $null
                try { $text = [string]$shape.TextFrame.TextRange.Text } catch {}
                $rows += [ordered]@{
                    role = $role
                    shape_id = [int]$shape.ID
                    shape_index = $i
                    shape_type = [int]$shape.Type
                    name = [string]$shape.Name
                    text = $text
                    left = [double]$shape.Left
                    top = [double]$shape.Top
                    width = [double]$shape.Width
                    height = [double]$shape.Height
                    rotation = [double]$shape.Rotation
                    z_order_position = [int]$shape.ZOrderPosition
                }
            }
        }
        finally { Release-Com $shape }
    }
    return @($rows)
}

function Get-LayoutGuideSnapshot {
    param($Owner)
    $guides = $null
    try {
        $guides = $Owner.LayoutGuides
        return [ordered]@{
            mirror_guides = Get-OptionalValue { [bool]$guides.MirrorGuides }
            margin_left = Get-OptionalValue { [double]$guides.MarginLeft }
            margin_right = Get-OptionalValue { [double]$guides.MarginRight }
            margin_top = Get-OptionalValue { [double]$guides.MarginTop }
            margin_bottom = Get-OptionalValue { [double]$guides.MarginBottom }
            columns = Get-OptionalValue { [int]$guides.Columns }
            rows = Get-OptionalValue { [int]$guides.Rows }
        }
    }
    finally { Release-Com $guides }
}

function Get-MasterPageSnapshot {
    param($Page,[int]$Index)
    return [ordered]@{
        index = $Index
        page_id = [int]$Page.PageID
        page_number = [string]$Page.PageNumber
        name = Get-OptionalValue { [string]$Page.Name }
        is_two_page_master = Get-OptionalValue { [bool]$Page.IsTwoPageMaster }
        is_leading = Get-OptionalValue { [bool]$Page.IsLeading }
        is_trailing = Get-OptionalValue { [bool]$Page.IsTrailing }
        width = [double]$Page.Width
        height = [double]$Page.Height
        layout_guides = Get-LayoutGuideSnapshot -Owner $Page
        tagged_shapes = @(Get-TaggedShapeSnapshots -Page $Page)
    }
}

function Get-PublicationPageSnapshot {
    param($Page,[int]$Index)
    $master = $null
    try {
        $master = $Page.Master
        return [ordered]@{
            index = $Index
            page_id = [int]$Page.PageID
            page_number = [string]$Page.PageNumber
            ignore_master = [bool]$Page.IgnoreMaster
            master_page_id = [int]$master.PageID
            reader_spread = Get-OptionalValue { [int]$Page.ReaderSpread }
            x_offset_within_reader_spread = Get-OptionalValue { [double]$Page.XOffsetWithinReaderSpread }
            y_offset_within_reader_spread = Get-OptionalValue { [double]$Page.YOffsetWithinReaderSpread }
            tagged_shapes = @(Get-TaggedShapeSnapshots -Page $Page)
        }
    }
    finally { Release-Com $master }
}

function Get-DocumentSnapshot {
    param($Document)
    $masters = @()
    for ($i=1; $i -le [int]$Document.MasterPages.Count; $i++) {
        $page = $null
        try {
            $page = $Document.MasterPages.Item($i)
            $masters += Get-MasterPageSnapshot -Page $page -Index $i
        }
        finally { Release-Com $page }
    }

    $pages = @()
    for ($i=1; $i -le [int]$Document.Pages.Count; $i++) {
        $page = $null
        try {
            $page = $Document.Pages.Item($i)
            $pages += Get-PublicationPageSnapshot -Page $page -Index $i
        }
        finally { Release-Com $page }
    }

    return [ordered]@{
        master_page_count = [int]$Document.MasterPages.Count
        publication_page_count = [int]$Document.Pages.Count
        document_layout_guides = Get-LayoutGuideSnapshot -Owner $Document
        masters = @($masters)
        pages = @($pages)
    }
}

function Add-TaggedRectangle {
    param($Page,[string]$Role,[double]$Left,[double]$Top,[double]$Width,[double]$Height,[int]$Rgb)
    $shape = $null
    try {
        $shape = $Page.Shapes.AddShape($MsoShapeRectangle,$Left,$Top,$Width,$Height)
        try { $shape.Name = "T825_" + $Role.Replace("-","_") } catch {}
        $shape.Tags.Add($TagName,$Role) | Out-Null
        $shape.Fill.Solid()
        $shape.Fill.ForeColor.RGB = $Rgb
        try { $shape.Fill.Transparency = 0 } catch {}
        $shape.Line.Visible = 0
        return [int]$shape.ID
    }
    finally { Release-Com $shape }
}

function Add-TaggedTextBox {
    param($Page,[string]$Role,[string]$Text,[double]$Left,[double]$Top,[double]$Width,[double]$Height)
    $shape = $null
    $range = $null
    try {
        $shape = $Page.Shapes.AddTextbox($MsoTextOrientationHorizontal,$Left,$Top,$Width,$Height)
        try { $shape.Name = "T825_" + $Role.Replace("-","_") } catch {}
        $shape.Tags.Add($TagName,$Role) | Out-Null
        $range = $shape.TextFrame.TextRange
        $range.Text = $Text
        try { $range.Font.Name = "Arial" } catch {}
        try { $range.Font.Size = 16 } catch {}
        return [int]$shape.ID
    }
    finally {
        Release-Com $range
        Release-Com $shape
    }
}

function Save-TrackedPageRenders {
    param($Document,[int[]]$TrackedPageIds,[string]$StageDir)
    $renderDir = Join-Path $StageDir "renders"
    New-Item -ItemType Directory -Force -Path $renderDir | Out-Null
    $rows = [ordered]@{}
    foreach ($pageId in $TrackedPageIds) {
        $page = $null
        try {
            $page = Find-PublicationPageById -Document $Document -PageId $pageId
            $path = Join-Path $renderDir ("page-" + $pageId + ".png")
            if (Test-Path -LiteralPath $path) { Remove-Item -LiteralPath $path -Force }
            $page.SaveAsPicture($path)
            if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
                throw "Page.SaveAsPicture did not create $path"
            }
            $rows[[string]$pageId] = Get-FileSummary $path
        }
        finally { Release-Com $page }
    }
    return $rows
}

function Snapshot-PubFile {
    param([string]$PubPath,[string]$StageDir,[int[]]$TrackedPageIds)
    New-Item -ItemType Directory -Force -Path $StageDir | Out-Null
    $app = $null
    $doc = $null
    try {
        $app = New-PubPublisherApplication
        $doc = $app.Open($PubPath,$true,$false)
        $semantic = Get-DocumentSnapshot -Document $doc
        $renders = Save-TrackedPageRenders -Document $doc -TrackedPageIds $TrackedPageIds -StageDir $StageDir
        return [ordered]@{
            semantic = $semantic
            renders = $renders
        }
    }
    finally {
        Close-Document $doc
        Close-PubPublisherApplication $app
    }
}

function New-Baseline {
    param([string]$SourceFixture,[string]$BaselinePub)

    $dir = Split-Path -Parent $BaselinePub
    New-Item -ItemType Directory -Force -Path $dir | Out-Null
    $working = Join-Path $dir "working.pub"
    Copy-Item -LiteralPath $SourceFixture -Destination $working -Force

    $app = $null
    $doc = $null
    $dedicatedMaster = $null
    $tracked = @()
    try {
        $app = New-PubPublisherApplication
        $doc = $app.Open($working,$false,$false)

        $doc.LayoutGuides.MirrorGuides = $false
        $doc.LayoutGuides.MarginLeft = 48
        $doc.LayoutGuides.MarginRight = 96
        $doc.LayoutGuides.MarginTop = 36
        $doc.LayoutGuides.MarginBottom = 72

        while ([int]$doc.Pages.Count -lt 4) {
            $created = $null
            try {
                $created = $doc.Pages.Add(1,[int]$doc.Pages.Count,-1,$false)
            }
            finally { Release-Com $created }
        }

        $dedicatedMaster = $doc.MasterPages.Add($false,"Z","T825 facing authority")
        $masterId = [int]$dedicatedMaster.PageID
        if ([bool]$dedicatedMaster.IsTwoPageMaster) {
            throw "Dedicated T825 master unexpectedly starts as two-page."
        }

        # Make the dedicated single-page master itself asymmetric before conversion.
        # This is separate from Document.LayoutGuides.MirrorGuides, which remains false.
        $dedicatedMaster.LayoutGuides.MarginLeft = 48
        $dedicatedMaster.LayoutGuides.MarginRight = 96
        $dedicatedMaster.LayoutGuides.MarginTop = 36
        $dedicatedMaster.LayoutGuides.MarginBottom = 72

        [void](Add-TaggedRectangle -Page $dedicatedMaster -Role "MASTER-LEFT-RECT" -Left 54 -Top 90 -Width 108 -Height 72 -Rgb 0x000000FF)
        [void](Add-TaggedRectangle -Page $dedicatedMaster -Role "MASTER-RIGHT-RECT" -Left 360 -Top 198 -Width 108 -Height 72 -Rgb 0x0000FF00)
        [void](Add-TaggedTextBox -Page $dedicatedMaster -Role "MASTER-TEXT" -Text "T825 MASTER ASYMMETRIC" -Left 180 -Top 54 -Width 216 -Height 36)

        for ($i=1; $i -le 4; $i++) {
            $page = $null
            try {
                $page = $doc.Pages.Item($i)
                $page.IgnoreMaster = $false
                $page.Master = $dedicatedMaster
                $tracked += [int]$page.PageID
                $localLeft = 36 + (24 * $i)
                [void](Add-TaggedRectangle -Page $page -Role ("CUSTOMER-" + $i) -Left $localLeft -Top 360 -Width 30 -Height 30 -Rgb 0x00FF0000)
            }
            finally { Release-Com $page }
        }

        $beforeSave = Get-DocumentSnapshot -Document $doc
        $doc.SaveAs($BaselinePub,$PbFilePublication,$false)
    }
    finally {
        Release-Com $dedicatedMaster
        Close-Document $doc
        Close-PubPublisherApplication $app
    }

    $fresh = Snapshot-PubFile -PubPath $BaselinePub -StageDir $dir -TrackedPageIds ([int[]]$tracked)
    $masterRows = @($fresh.semantic.masters | Where-Object { [int]$_.page_id -eq $masterId })
    if ($masterRows.Count -ne 1) { throw "Baseline dedicated master identity was not preserved." }
    foreach ($pageId in $tracked) {
        $rows = @($fresh.semantic.pages | Where-Object { [int]$_.page_id -eq $pageId })
        if ($rows.Count -ne 1 -or [int]$rows[0].master_page_id -ne $masterId) {
            throw "Baseline tracked page $pageId is not bound to dedicated master $masterId."
        }
    }

    return [ordered]@{
        output = Get-FileSummary $BaselinePub
        dedicated_master_page_id = $masterId
        tracked_customer_page_ids = @($tracked)
        before_save = $beforeSave
        fresh_reopen = $fresh
    }
}

function Invoke-NoopControl {
    param([string]$BaselinePub,[int[]]$TrackedPageIds)
    $dir = Join-Path $privateDir "control"
    New-Item -ItemType Directory -Force -Path $dir | Out-Null
    $output = Join-Path $dir "control.pub"
    Copy-Item -LiteralPath $BaselinePub -Destination $output -Force
    $app=$null; $doc=$null
    try {
        $app=New-PubPublisherApplication
        $doc=$app.Open($output,$false,$false)
        $before=Get-DocumentSnapshot -Document $doc
        $doc.Save()
    }
    finally {
        Close-Document $doc
        Close-PubPublisherApplication $app
    }
    $fresh=Snapshot-PubFile -PubPath $output -StageDir $dir -TrackedPageIds $TrackedPageIds
    return [ordered]@{
        output=Get-FileSummary $output
        before_save=$before
        fresh_reopen=$fresh
    }
}

function Invoke-Convert {
    param([string]$BaselinePub,[int]$MasterPageId,[int[]]$TrackedPageIds)
    $dir = Join-Path $privateDir "convert-two-page"
    New-Item -ItemType Directory -Force -Path $dir | Out-Null
    $output = Join-Path $dir "converted.pub"
    Copy-Item -LiteralPath $BaselinePub -Destination $output -Force
    $app=$null; $doc=$null; $master=$null
    try {
        $app=New-PubPublisherApplication
        $doc=$app.Open($output,$false,$false)
        $before=Get-DocumentSnapshot -Document $doc
        $master=Find-MasterPageById -Document $doc -PageId $MasterPageId
        $master.IsTwoPageMaster = $true
        $readback=[bool]$master.IsTwoPageMaster
        $afterMutation=Get-DocumentSnapshot -Document $doc
        $doc.Save()
    }
    finally {
        Release-Com $master
        Close-Document $doc
        Close-PubPublisherApplication $app
    }
    $fresh=Snapshot-PubFile -PubPath $output -StageDir $dir -TrackedPageIds $TrackedPageIds
    return [ordered]@{
        mutation=[ordered]@{ requested=$true; readback=$readback }
        output=Get-FileSummary $output
        before_mutation=$before
        after_mutation=$afterMutation
        fresh_reopen=$fresh
    }
}

function Invoke-ParityInsert {
    param([string]$ConvertedPub,[int]$MasterPageId,[int[]]$TrackedPageIds)
    $dir = Join-Path $privateDir "parity-insert"
    New-Item -ItemType Directory -Force -Path $dir | Out-Null
    $output = Join-Path $dir "parity-insert.pub"
    Copy-Item -LiteralPath $ConvertedPub -Destination $output -Force
    $app=$null; $doc=$null; $inserted=$null; $master=$null
    $newPageId=$null
    try {
        $app=New-PubPublisherApplication
        $doc=$app.Open($output,$false,$false)
        $before=Get-DocumentSnapshot -Document $doc
        $inserted=$doc.Pages.Add(1,0,-1,$false)
        $newPageId=[int]$inserted.PageID
        try {
            $master=Find-MasterPageById -Document $doc -PageId $MasterPageId
            $inserted.Master=$master
            $inserted.IgnoreMaster=$false
        }
        finally { Release-Com $master }
        $afterMutation=Get-DocumentSnapshot -Document $doc
        $doc.Save()
    }
    finally {
        Release-Com $inserted
        Close-Document $doc
        Close-PubPublisherApplication $app
    }
    $fresh=Snapshot-PubFile -PubPath $output -StageDir $dir -TrackedPageIds $TrackedPageIds
    return [ordered]@{
        mutation=[ordered]@{
            kind="insert-one-page-before-tracked-set"
            inserted_page_id=$newPageId
            original_tracked_page_ids=@($TrackedPageIds)
        }
        output=Get-FileSummary $output
        before_mutation=$before
        after_mutation=$afterMutation
        fresh_reopen=$fresh
    }
}

function Invoke-ConvertBack {
    param([string]$ConvertedPub,[int]$MasterPageId,[int[]]$TrackedPageIds)
    $dir = Join-Path $privateDir "convert-back"
    New-Item -ItemType Directory -Force -Path $dir | Out-Null
    $output = Join-Path $dir "convert-back.pub"
    Copy-Item -LiteralPath $ConvertedPub -Destination $output -Force
    $app=$null; $doc=$null; $master=$null
    $supported=$false; $errorRecord=$null; $readback=$null
    try {
        $app=New-PubPublisherApplication
        $doc=$app.Open($output,$false,$false)
        $before=Get-DocumentSnapshot -Document $doc
        try {
            $master=Find-MasterPageById -Document $doc -PageId $MasterPageId
            $master.IsTwoPageMaster = $false
            $readback=[bool]$master.IsTwoPageMaster
            $supported=$true
            $afterMutation=Get-DocumentSnapshot -Document $doc
            $doc.Save()
        }
        catch {
            $errorRecord=[ordered]@{
                hresult=("0x{0:X8}" -f ($_.Exception.HResult -band 0xffffffff))
                message=$_.Exception.Message
            }
            $afterMutation=Get-DocumentSnapshot -Document $doc
        }
    }
    finally {
        Release-Com $master
        Close-Document $doc
        Close-PubPublisherApplication $app
    }

    $fresh=Snapshot-PubFile -PubPath $output -StageDir $dir -TrackedPageIds $TrackedPageIds
    return [ordered]@{
        mutation=[ordered]@{
            requested=$false
            supported=$supported
            readback=$readback
            error=$errorRecord
        }
        output=Get-FileSummary $output
        before_mutation=$before
        after_mutation=$afterMutation
        fresh_reopen=$fresh
    }
}

$fixturePath=[string]$env:PUB_RESEARCH_FIXTURE
if ([string]::IsNullOrWhiteSpace($fixturePath) -or -not (Test-Path -LiteralPath $fixturePath -PathType Leaf)) {
    throw "T825 requires packet-prepared PUB_RESEARCH_FIXTURE."
}
$fixtureSha=Get-Sha256 $fixturePath
if ($fixtureSha -ne $ExpectedFixtureSha256) {
    throw "T825 fixture SHA mismatch: expected $ExpectedFixtureSha256 got $fixtureSha"
}

$baselinePub=Join-Path $privateDir "baseline/baseline.pub"
$baseline=New-Baseline -SourceFixture $fixturePath -BaselinePub $baselinePub
$tracked=[int[]]@($baseline.tracked_customer_page_ids)
$masterId=[int]$baseline.dedicated_master_page_id

$control=Invoke-NoopControl -BaselinePub $baselinePub -TrackedPageIds $tracked
$converted=Invoke-Convert -BaselinePub $baselinePub -MasterPageId $masterId -TrackedPageIds $tracked
$parity=Invoke-ParityInsert -ConvertedPub (Join-Path $privateDir "convert-two-page/converted.pub") -MasterPageId $masterId -TrackedPageIds $tracked
$back=Invoke-ConvertBack -ConvertedPub (Join-Path $privateDir "convert-two-page/converted.pub") -MasterPageId $masterId -TrackedPageIds $tracked

$result=[ordered]@{
    schema="chaptera.publisher.master-facing-layout-auth-01.native.v1"
    experiment_id=$ExperimentId
    publisher_target=[ordered]@{
        version_prefix="16.0.12527."
        exe_sha256="e1ef8811b85b82045f37c4173b92726101be3a25e550b0dcb9f178df834ab20b"
    }
    fixture=[ordered]@{
        expected_sha256=$ExpectedFixtureSha256
        discovered_sha256=$fixtureSha
        source_is_immutable=$true
    }
    baseline=$baseline
    arms=[ordered]@{
        control=$control
        convert_two_page=$converted
        parity_insert=$parity
        convert_back=$back
    }
    documented_distinctions=[ordered]@{
        page_is_two_page_master="read/write master-page property; experiment mutates only the dedicated master"
        document_mirror_guides="observed only; experiment does not set MirrorGuides=True in the conversion arm"
    }
    boundary="Publisher 2019/build12527; print publication; one dedicated master; four tracked customer pages; IsTwoPageMaster conversion, topology, projection handedness and one ordinal parity discriminator only. No booklet imposition, section-number authority, IgnoreMaster, master lifecycle beyond the conversion-induced partner, or Chaptera implementation."
}

Write-PubJson -Value $result -Path (Join-Path $analysisDir "master-facing-layout-auth-01.json")
@(
    "experiment=$ExperimentId",
    "fixture_sha256=$fixtureSha",
    "dedicated_master_page_id=$masterId",
    "tracked_customer_page_ids=$($tracked -join ',')",
    "baseline_master_count=$($baseline.fresh_reopen.semantic.master_page_count)",
    "converted_master_count=$($converted.fresh_reopen.semantic.master_page_count)",
    "converted_document_mirror_guides=$($converted.fresh_reopen.semantic.document_layout_guides.mirror_guides.value)",
    "convert_back_supported=$($back.mutation.supported)"
) | Set-Content -LiteralPath (Join-Path $logDir "master-facing-layout-auth-01.txt") -Encoding ASCII
