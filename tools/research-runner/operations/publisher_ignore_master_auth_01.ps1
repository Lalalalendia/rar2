param(
    [Parameter(Mandatory = $true)][string]$PacketPath,
    [Parameter(Mandatory = $true)][string]$OutputRoot
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$ExpectedExperiment = "IGNORE-MASTER-AUTH-01"
$ExpectedFixtureSha256 = "a8598bd40b32491b774df0aad11dda11681cc624c0a6edb22e59b14de8904364"
$PageAId = 33554698
$PageBId = 33554737
$Master1Id = 33554695
$Master2Id = 33554741
$PbFilePublication = 1
$MsoTextOrientationHorizontal = 1
$TagName = "PUB_T828_ROLE"

$packet = Get-Content -LiteralPath $PacketPath -Raw | ConvertFrom-Json
if ([string]$packet.id -ne $ExpectedExperiment) {
    throw "Unexpected experiment id: $($packet.id)"
}

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "../../..")).Path
Import-Module (Join-Path $repoRoot "tools/windows/pub-runtime/PubRuntime.psm1") -Force

$analysisDir = Join-Path $OutputRoot "analysis"
$logDir = Join-Path $OutputRoot "logs"
$privateDir = Join-Path $OutputRoot "private/ignore-master-auth-01"
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

function Get-Sha256([string]$Path) {
    return (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
}

function Get-RelativeOutputPath([string]$Path) {
    $full = [System.IO.Path]::GetFullPath($Path)
    $root = [System.IO.Path]::GetFullPath($OutputRoot).TrimEnd([char]'\',[char]'/') + [System.IO.Path]::DirectorySeparatorChar
    if ($full.StartsWith($root, [System.StringComparison]::OrdinalIgnoreCase)) {
        return $full.Substring($root.Length).Replace('\','/')
    }
    return [System.IO.Path]::GetFileName($full)
}

function Get-FileSummary([string]$Path) {
    $item = Get-Item -LiteralPath $Path
    return [ordered]@{
        relative_path = Get-RelativeOutputPath $Path
        size = [int64]$item.Length
        sha256 = Get-Sha256 $Path
    }
}

function Find-RegisteredFixture {
    $candidate = Join-Path $repoRoot "realtest/master2-native-20260924/output/two-master.pub"
    if (Test-Path -LiteralPath $candidate -PathType Leaf) {
        if ((Get-Sha256 $candidate) -eq $ExpectedFixtureSha256) {
            return (Resolve-Path -LiteralPath $candidate).Path
        }
    }

    $realtest = Join-Path $repoRoot "realtest"
    if (Test-Path -LiteralPath $realtest -PathType Container) {
        foreach ($file in Get-ChildItem -LiteralPath $realtest -Filter *.pub -File -Recurse) {
            try {
                if ((Get-Sha256 $file.FullName) -eq $ExpectedFixtureSha256) {
                    return $file.FullName
                }
            }
            catch {}
        }
    }

    throw "Registered PUB-T-462 fixture not found under realtest. Expected SHA-256 $ExpectedFixtureSha256."
}

function Find-PublicationPageById {
    param(
        [Parameter(Mandatory = $true)]$Document,
        [Parameter(Mandatory = $true)][int]$PageId
    )
    for ($i = 1; $i -le [int]$Document.Pages.Count; $i++) {
        $page = $null
        try {
            $page = $Document.Pages.Item($i)
            if ([int]$page.PageID -eq $PageId) {
                return $page
            }
        }
        finally {
            if ($null -ne $page -and [int]$page.PageID -ne $PageId) {
                Release-Com $page
            }
        }
    }
    throw "Publication page PageID=$PageId not found."
}

function Find-MasterPageById {
    param(
        [Parameter(Mandatory = $true)]$Document,
        [Parameter(Mandatory = $true)][int]$PageId
    )
    for ($i = 1; $i -le [int]$Document.MasterPages.Count; $i++) {
        $page = $null
        try {
            $page = $Document.MasterPages.Item($i)
            if ([int]$page.PageID -eq $PageId) {
                return $page
            }
        }
        finally {
            if ($null -ne $page -and [int]$page.PageID -ne $PageId) {
                Release-Com $page
            }
        }
    }
    throw "Master page PageID=$PageId not found."
}

function Get-TaggedShapeSnapshots {
    param([Parameter(Mandatory = $true)]$Page)

    $rows = @()
    for ($shapeIndex = 1; $shapeIndex -le [int]$Page.Shapes.Count; $shapeIndex++) {
        $shape = $null
        try {
            $shape = $Page.Shapes.Item($shapeIndex)
            $role = $null
            for ($tagIndex = 1; $tagIndex -le [int]$shape.Tags.Count; $tagIndex++) {
                $tag = $null
                try {
                    $tag = $shape.Tags.Item($tagIndex)
                    if ([string]$tag.Name -eq $TagName) {
                        $role = [string]$tag.Value
                    }
                }
                finally {
                    Release-Com $tag
                }
            }

            if ($null -ne $role) {
                $text = $null
                try { $text = [string]$shape.TextFrame.TextRange.Text } catch {}
                $rows += [ordered]@{
                    role = $role
                    shape_index = $shapeIndex
                    shape_id = [int]$shape.ID
                    shape_type = [int]$shape.Type
                    name = [string]$shape.Name
                    text = $text
                    left = [double]$shape.Left
                    top = [double]$shape.Top
                    width = [double]$shape.Width
                    height = [double]$shape.Height
                }
            }
        }
        finally {
            Release-Com $shape
        }
    }
    return @($rows)
}

function Get-PageSnapshot {
    param(
        [Parameter(Mandatory = $true)]$Document,
        [Parameter(Mandatory = $true)][int]$PageId
    )

    $page = $null
    $master = $null
    try {
        $page = Find-PublicationPageById -Document $Document -PageId $PageId
        $master = $page.Master
        $masterName = $null
        try { $masterName = [string]$master.Name } catch {}
        return [ordered]@{
            page_id = [int]$page.PageID
            page_number = [int]$page.PageNumber
            ignore_master = [bool]$page.IgnoreMaster
            master_page_id = [int]$master.PageID
            master_name = $masterName
            tagged_shapes = @(Get-TaggedShapeSnapshots -Page $page)
        }
    }
    finally {
        Release-Com $master
        Release-Com $page
    }
}

function Get-MasterSnapshot {
    param(
        [Parameter(Mandatory = $true)]$Document,
        [Parameter(Mandatory = $true)][int]$PageId
    )
    $page = $null
    try {
        $page = Find-MasterPageById -Document $Document -PageId $PageId
        $name = $null
        try { $name = [string]$page.Name } catch {}
        return [ordered]@{
            page_id = [int]$page.PageID
            name = $name
            tagged_shapes = @(Get-TaggedShapeSnapshots -Page $page)
        }
    }
    finally {
        Release-Com $page
    }
}

function Get-DocumentSnapshot {
    param([Parameter(Mandatory = $true)]$Document)
    return [ordered]@{
        publication_page_count = [int]$Document.Pages.Count
        master_page_count = [int]$Document.MasterPages.Count
        page_a = Get-PageSnapshot -Document $Document -PageId $PageAId
        page_b = Get-PageSnapshot -Document $Document -PageId $PageBId
        master_1 = Get-MasterSnapshot -Document $Document -PageId $Master1Id
        master_2 = Get-MasterSnapshot -Document $Document -PageId $Master2Id
    }
}

function Add-TaggedTextBox {
    param(
        [Parameter(Mandatory = $true)]$Page,
        [Parameter(Mandatory = $true)][string]$Role,
        [Parameter(Mandatory = $true)][string]$Text,
        [Parameter(Mandatory = $true)][double]$Left,
        [Parameter(Mandatory = $true)][double]$Top
    )

    $shape = $null
    $range = $null
    try {
        $shape = $Page.Shapes.AddTextbox($MsoTextOrientationHorizontal, $Left, $Top, 180, 36)
        $shape.Tags.Add($TagName, $Role) | Out-Null
        $range = $shape.TextFrame.TextRange
        $range.Text = $Text
        try { $range.Font.Name = "Arial" } catch {}
        try { $range.Font.Size = 18 } catch {}
        return [int]$shape.ID
    }
    finally {
        Release-Com $range
        Release-Com $shape
    }
}

function Save-PageRender {
    param(
        [Parameter(Mandatory = $true)]$Document,
        [Parameter(Mandatory = $true)][int]$PageId,
        [Parameter(Mandatory = $true)][string]$Path
    )
    $page = $null
    try {
        $page = Find-PublicationPageById -Document $Document -PageId $PageId
        if (Test-Path -LiteralPath $Path) {
            Remove-Item -LiteralPath $Path -Force
        }
        $page.SaveAsPicture($Path)
        if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
            throw "Page.SaveAsPicture did not create $Path"
        }
    }
    finally {
        Release-Com $page
    }
}

function Snapshot-PubFile {
    param(
        [Parameter(Mandatory = $true)][string]$PubPath,
        [Parameter(Mandatory = $true)][string]$StageDir
    )

    New-Item -ItemType Directory -Force -Path $StageDir | Out-Null
    $pageARender = Join-Path $StageDir "page-a.png"
    $pageBRender = Join-Path $StageDir "page-b.png"

    $app = $null
    $doc = $null
    try {
        $app = New-PubPublisherApplication
        $doc = $app.Open($PubPath, $true, $false)
        $semantic = Get-DocumentSnapshot -Document $doc
        Save-PageRender -Document $doc -PageId $PageAId -Path $pageARender
        Save-PageRender -Document $doc -PageId $PageBId -Path $pageBRender
        return [ordered]@{
            semantic = $semantic
            renders = [ordered]@{
                page_a = Get-FileSummary $pageARender
                page_b = Get-FileSummary $pageBRender
            }
        }
    }
    finally {
        Close-Document $doc
        Close-PubPublisherApplication $app
    }
}

function Get-RoleShapeId {
    param(
        [Parameter(Mandatory = $true)]$DocumentSnapshot,
        [Parameter(Mandatory = $true)][string]$Carrier,
        [Parameter(Mandatory = $true)][string]$Role
    )
    $carrierSnapshot = $DocumentSnapshot[$Carrier]
    $matches = @($carrierSnapshot.tagged_shapes | Where-Object { [string]$_.role -eq $Role })
    if ($matches.Count -ne 1) {
        throw "Expected exactly one role=$Role on carrier=$Carrier; found $($matches.Count)."
    }
    return [int]$matches[0].shape_id
}

function New-SeedFixture {
    param(
        [Parameter(Mandatory = $true)][string]$SourceFixture,
        [Parameter(Mandatory = $true)][string]$SeedPub
    )

    $seedDir = Split-Path -Parent $SeedPub
    New-Item -ItemType Directory -Force -Path $seedDir | Out-Null
    $working = Join-Path $seedDir "seed-working.pub"
    Copy-Item -LiteralPath $SourceFixture -Destination $working -Force

    $app = $null
    $doc = $null
    $pageA = $null
    $pageB = $null
    $master1 = $null
    $master2 = $null
    try {
        $app = New-PubPublisherApplication
        $doc = $app.Open($working, $false, $false)
        $pageA = Find-PublicationPageById -Document $doc -PageId $PageAId
        $pageB = Find-PublicationPageById -Document $doc -PageId $PageBId
        $master1 = Find-MasterPageById -Document $doc -PageId $Master1Id
        $master2 = Find-MasterPageById -Document $doc -PageId $Master2Id

        $aMaster = $null
        $bMaster = $null
        try {
            $aMaster = $pageA.Master
            $bMaster = $pageB.Master
            if ([int]$aMaster.PageID -ne $Master1Id) {
                throw "PUB-T-462 precondition failed: Page A is not initially bound to Master 1."
            }
            if ([int]$bMaster.PageID -ne $Master2Id) {
                throw "PUB-T-462 precondition failed: Page B is not initially bound to Master 2."
            }
        }
        finally {
            Release-Com $bMaster
            Release-Com $aMaster
        }

        $pageA.IgnoreMaster = $false
        $pageB.IgnoreMaster = $false
        $pageB.Master = $master1

        [void](Add-TaggedTextBox -Page $master1 -Role "MASTER-ONE" -Text "T828 MASTER ONE" -Left 72 -Top 72)
        [void](Add-TaggedTextBox -Page $master1 -Role "MASTER-TWO" -Text "T828 MASTER TWO" -Left 72 -Top 120)
        [void](Add-TaggedTextBox -Page $pageA -Role "PAGE-A-LOCAL" -Text "T828 PAGE A LOCAL" -Left 306 -Top 72)
        [void](Add-TaggedTextBox -Page $pageB -Role "PAGE-B-LOCAL" -Text "T828 PAGE B LOCAL" -Left 306 -Top 72)

        $doc.SaveAs($SeedPub, $PbFilePublication, $false)
    }
    finally {
        Release-Com $master2
        Release-Com $master1
        Release-Com $pageB
        Release-Com $pageA
        Close-Document $doc
        Close-PubPublisherApplication $app
    }

    $fresh = Snapshot-PubFile -PubPath $SeedPub -StageDir $seedDir
    if ([bool]$fresh.semantic.page_a.ignore_master -or [bool]$fresh.semantic.page_b.ignore_master) {
        throw "Seed normalization failed: IgnoreMaster must be false on A and B."
    }
    if ([int]$fresh.semantic.page_a.master_page_id -ne $Master1Id -or [int]$fresh.semantic.page_b.master_page_id -ne $Master1Id) {
        throw "Seed normalization failed: A and B must both bind to Master 1."
    }
    if (@($fresh.semantic.master_1.tagged_shapes).Count -lt 2) {
        throw "Seed normalization failed: Master 1 must retain two tagged master-owned shapes."
    }
    return [ordered]@{
        output = Get-FileSummary $SeedPub
        fresh_reopen = $fresh
    }
}

function Invoke-SimpleArm {
    param(
        [Parameter(Mandatory = $true)][string]$Name,
        [Parameter(Mandatory = $true)][string]$SeedPub,
        [Parameter(Mandatory = $true)][scriptblock]$Mutation
    )

    $armDir = Join-Path $privateDir ("arms/" + $Name)
    New-Item -ItemType Directory -Force -Path $armDir | Out-Null
    $output = Join-Path $armDir "output.pub"
    Copy-Item -LiteralPath $SeedPub -Destination $output -Force

    $app = $null
    $doc = $null
    $before = $null
    $after = $null
    $mutationReceipt = $null
    try {
        $app = New-PubPublisherApplication
        $doc = $app.Open($output, $false, $false)
        $before = Get-DocumentSnapshot -Document $doc
        $mutationReceipt = & $Mutation $doc
        $after = Get-DocumentSnapshot -Document $doc
        $doc.Save()
    }
    finally {
        Close-Document $doc
        Close-PubPublisherApplication $app
    }

    $fresh = Snapshot-PubFile -PubPath $output -StageDir $armDir
    return [ordered]@{
        arm = $Name
        mutation = $mutationReceipt
        before_mutation = $before
        after_mutation = $after
        output = Get-FileSummary $output
        fresh_reopen = $fresh
    }
}

function Invoke-ReversibleArm {
    param([Parameter(Mandatory = $true)][string]$SeedPub)

    $name = "reversible"
    $armDir = Join-Path $privateDir ("arms/" + $name)
    New-Item -ItemType Directory -Force -Path $armDir | Out-Null
    $output = Join-Path $armDir "output.pub"
    $afterTruePub = Join-Path $armDir "after-true.pub"
    Copy-Item -LiteralPath $SeedPub -Destination $output -Force

    $app = $null
    $doc = $null
    $pageA = $null
    try {
        $app = New-PubPublisherApplication
        $doc = $app.Open($output, $false, $false)
        $pageA = Find-PublicationPageById -Document $doc -PageId $PageAId
        $pageA.IgnoreMaster = $true
        $doc.Save()
    }
    finally {
        Release-Com $pageA
        Close-Document $doc
        Close-PubPublisherApplication $app
    }
    Copy-Item -LiteralPath $output -Destination $afterTruePub -Force
    $afterTrue = Snapshot-PubFile -PubPath $afterTruePub -StageDir (Join-Path $armDir "after-true")

    $app2 = $null
    $doc2 = $null
    $pageA2 = $null
    try {
        $app2 = New-PubPublisherApplication
        $doc2 = $app2.Open($output, $false, $false)
        $pageA2 = Find-PublicationPageById -Document $doc2 -PageId $PageAId
        if (-not [bool]$pageA2.IgnoreMaster) {
            throw "Reversible arm failed: IgnoreMaster=True did not survive the first reopen."
        }
        $pageA2.IgnoreMaster = $false
        $doc2.Save()
    }
    finally {
        Release-Com $pageA2
        Close-Document $doc2
        Close-PubPublisherApplication $app2
    }

    $final = Snapshot-PubFile -PubPath $output -StageDir (Join-Path $armDir "final-false")
    return [ordered]@{
        arm = $name
        mutation = [ordered]@{ sequence = @("IgnoreMaster=True","Save/reopen","IgnoreMaster=False","Save/reopen") }
        after_true = [ordered]@{
            output = Get-FileSummary $afterTruePub
            fresh_reopen = $afterTrue
        }
        final_false = [ordered]@{
            output = Get-FileSummary $output
            fresh_reopen = $final
        }
    }
}

function Role-Identity-Stable {
    param(
        [Parameter(Mandatory = $true)]$SeedSnapshot,
        [Parameter(Mandatory = $true)]$CandidateSnapshot
    )
    $checks = @(
        @("page_a","PAGE-A-LOCAL"),
        @("page_b","PAGE-B-LOCAL"),
        @("master_1","MASTER-ONE"),
        @("master_1","MASTER-TWO")
    )
    foreach ($pair in $checks) {
        if ((Get-RoleShapeId -DocumentSnapshot $SeedSnapshot -Carrier $pair[0] -Role $pair[1]) -ne
            (Get-RoleShapeId -DocumentSnapshot $CandidateSnapshot -Carrier $pair[0] -Role $pair[1])) {
            return $false
        }
    }
    return $true
}

$fixturePath = Find-RegisteredFixture
$fixtureSummary = Get-FileSummary $fixturePath
if ([string]$fixtureSummary.sha256 -ne $ExpectedFixtureSha256) {
    throw "Registered fixture hash changed unexpectedly."
}

$seedPub = Join-Path $privateDir "seed/seed.pub"
$seed = New-SeedFixture -SourceFixture $fixturePath -SeedPub $seedPub

$control = Invoke-SimpleArm -Name "control" -SeedPub $seedPub -Mutation {
    param($doc)
    return [ordered]@{ kind = "noop" }
}

$ignoreTrue = Invoke-SimpleArm -Name "ignore-true" -SeedPub $seedPub -Mutation {
    param($doc)
    $pageA = $null
    try {
        $pageA = Find-PublicationPageById -Document $doc -PageId $PageAId
        $pageA.IgnoreMaster = $true
        return [ordered]@{ kind = "set-ignore-master"; requested = $true; readback = [bool]$pageA.IgnoreMaster }
    }
    finally {
        Release-Com $pageA
    }
}

$trueFalse = Invoke-SimpleArm -Name "true-false-before-save" -SeedPub $seedPub -Mutation {
    param($doc)
    $pageA = $null
    try {
        $pageA = Find-PublicationPageById -Document $doc -PageId $PageAId
        $pageA.IgnoreMaster = $true
        $readTrue = [bool]$pageA.IgnoreMaster
        $pageA.IgnoreMaster = $false
        $readFalse = [bool]$pageA.IgnoreMaster
        return [ordered]@{
            kind = "true-false-before-save"
            readback_true = $readTrue
            readback_false = $readFalse
        }
    }
    finally {
        Release-Com $pageA
    }
}

$reversible = Invoke-ReversibleArm -SeedPub $seedPub

$rebindTrue = Invoke-SimpleArm -Name "rebind-while-true" -SeedPub $seedPub -Mutation {
    param($doc)
    $pageA = $null
    $master2 = $null
    try {
        $pageA = Find-PublicationPageById -Document $doc -PageId $PageAId
        $master2 = Find-MasterPageById -Document $doc -PageId $Master2Id
        $pageA.IgnoreMaster = $true
        $pageA.Master = $master2
        $bound = $null
        try {
            $bound = $pageA.Master
            return [ordered]@{
                kind = "ignore-master-plus-rebind"
                ignore_master = [bool]$pageA.IgnoreMaster
                requested_master_page_id = $Master2Id
                readback_master_page_id = [int]$bound.PageID
            }
        }
        finally {
            Release-Com $bound
        }
    }
    finally {
        Release-Com $master2
        Release-Com $pageA
    }
}

$seedSemantic = $seed.fresh_reopen.semantic
$allFreshSnapshots = @(
    $control.fresh_reopen.semantic,
    $ignoreTrue.fresh_reopen.semantic,
    $trueFalse.fresh_reopen.semantic,
    $reversible.after_true.fresh_reopen.semantic,
    $reversible.final_false.fresh_reopen.semantic,
    $rebindTrue.fresh_reopen.semantic
)

$identityStable = $true
foreach ($snapshot in $allFreshSnapshots) {
    if (-not (Role-Identity-Stable -SeedSnapshot $seedSemantic -CandidateSnapshot $snapshot)) {
        $identityStable = $false
        break
    }
}

$checks = [ordered]@{
    seed_a_b_share_master_1 = (
        [int]$seedSemantic.page_a.master_page_id -eq $Master1Id -and
        [int]$seedSemantic.page_b.master_page_id -eq $Master1Id
    )
    control_false_and_master_1 = (
        -not [bool]$control.fresh_reopen.semantic.page_a.ignore_master -and
        [int]$control.fresh_reopen.semantic.page_a.master_page_id -eq $Master1Id
    )
    ignore_true_persists = [bool]$ignoreTrue.fresh_reopen.semantic.page_a.ignore_master
    ignore_true_preserves_master_1_binding = (
        [int]$ignoreTrue.fresh_reopen.semantic.page_a.master_page_id -eq $Master1Id
    )
    true_false_before_save_returns_false = (
        -not [bool]$trueFalse.fresh_reopen.semantic.page_a.ignore_master -and
        [int]$trueFalse.fresh_reopen.semantic.page_a.master_page_id -eq $Master1Id
    )
    reversible_true_stage_persists = (
        [bool]$reversible.after_true.fresh_reopen.semantic.page_a.ignore_master -and
        [int]$reversible.after_true.fresh_reopen.semantic.page_a.master_page_id -eq $Master1Id
    )
    reversible_final_false_persists = (
        -not [bool]$reversible.final_false.fresh_reopen.semantic.page_a.ignore_master -and
        [int]$reversible.final_false.fresh_reopen.semantic.page_a.master_page_id -eq $Master1Id
    )
    rebind_while_true_keeps_ignore_true = [bool]$rebindTrue.fresh_reopen.semantic.page_a.ignore_master
    rebind_while_true_persists_master_2 = (
        [int]$rebindTrue.fresh_reopen.semantic.page_a.master_page_id -eq $Master2Id
    )
    tagged_master_and_page_local_shape_identity_stable = [bool]$identityStable
}

$comClassification = "inconclusive"
if (
    $checks.control_false_and_master_1 -and
    $checks.ignore_true_persists -and
    $checks.ignore_true_preserves_master_1_binding -and
    $checks.true_false_before_save_returns_false -and
    $checks.reversible_true_stage_persists -and
    $checks.reversible_final_false_persists -and
    $checks.rebind_while_true_keeps_ignore_true -and
    $checks.rebind_while_true_persists_master_2 -and
    $checks.tagged_master_and_page_local_shape_identity_stable
) {
    $comClassification = "independent-ignore-master-and-applied-master-state-confirmed-at-com-layer"
}

$result = [ordered]@{
    schema = "chaptera.publisher.ignore-master-auth-01.native.v1"
    experiment_id = $ExpectedExperiment
    publisher_target = [ordered]@{
        version_prefix = "16.0.12527."
        exe_sha256 = "e1ef8811b85b82045f37c4173b92726101be3a25e550b0dcb9f178df834ab20b"
    }
    fixture = [ordered]@{
        artifact_id = "PUB-ART-639"
        task_fixture = "PUB-T-462"
        expected_sha256 = $ExpectedFixtureSha256
        discovered_sha256 = $fixtureSummary.sha256
        size = $fixtureSummary.size
        source_is_immutable = $true
    }
    page_ids = [ordered]@{
        page_a = $PageAId
        page_b = $PageBId
        master_1 = $Master1Id
        master_2 = $Master2Id
    }
    render_oracle = "Publisher Page.SaveAsPicture PNG captured only after fresh reopen."
    seed = $seed
    arms = [ordered]@{
        control = $control
        ignore_true = $ignoreTrue
        true_false_before_save = $trueFalse
        reversible = $reversible
        rebind_while_true = $rebindTrue
    }
    checks = $checks
    com_classification = $comClassification
    next_analysis = "Run ignore_master_auth_01_blast_radius.py. LAW71 must not close from COM alone: require fresh-reopen render suppression plus matched-control CFB/Contents evidence."
    boundary = "Bounded to Publisher 2019 build 16.0.12527, registered PUB-T-462 lineage, ordinary non-facing customer pages, static master/page-local tagged TextBoxes. No Two Page Master, HeaderFooter, section numbering, master lifecycle, or Chaptera authoring claim."
}

Write-PubJson -Value $result -Path (Join-Path $analysisDir "ignore-master-auth-01.json")
@(
    "experiment=$ExpectedExperiment",
    "fixture_sha256=$ExpectedFixtureSha256",
    "page_a=$PageAId",
    "page_b=$PageBId",
    "master_1=$Master1Id",
    "master_2=$Master2Id",
    "ignore_true_persists=$($checks.ignore_true_persists)",
    "ignore_true_preserves_master_1_binding=$($checks.ignore_true_preserves_master_1_binding)",
    "reversible_final_false_persists=$($checks.reversible_final_false_persists)",
    "rebind_while_true_persists_master_2=$($checks.rebind_while_true_persists_master_2)",
    "identity_stable=$($checks.tagged_master_and_page_local_shape_identity_stable)",
    "com_classification=$comClassification"
) | Set-Content -LiteralPath (Join-Path $logDir "ignore-master-auth-01.txt") -Encoding ASCII
