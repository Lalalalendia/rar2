param(
    [Parameter(Mandatory = $true)]
    [string]$PacketPath,
    [Parameter(Mandatory = $true)]
    [string]$OutputRoot
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$ExpectedExperiment = "VIEWER-SCHEME-COLOR-ORACLE-01"
$ExpectedFixtureSha256 = "5bf6057b8b11c8ee4a421d93885ae6e9e7c03a7a42d541e6d0df33497c08c33b"
$ExpectedPublisherExeSha256 = "e1ef8811b85b82045f37c4173b92726101be3a25e550b0dcb9f178df834ab20b"
$PbFilePublication = 1
$MsoShapeRectangle = 1
$MsoFalse = 0

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
    throw "Exact blank fixture mismatch: $fixtureHash"
}
$publisherHash = (Get-FileHash -LiteralPath $env:PUB_RESEARCH_PUBLISHER_EXE -Algorithm SHA256).Hash.ToLowerInvariant()
if ($publisherHash -ne $ExpectedPublisherExeSha256) {
    throw "Publisher executable SHA-256 mismatch: $publisherHash"
}

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "../../..")).Path
Import-Module (Join-Path $repoRoot "tools/windows/pub-runtime/PubRuntime.psm1") -Force

$analysisDir = Join-Path $OutputRoot "analysis"
$rawDir = Join-Path $analysisDir "raw"
$logDir = Join-Path $OutputRoot "logs"
$privateDir = Join-Path $OutputRoot "private/scheme-color-oracle-01"
New-Item -ItemType Directory -Force -Path $analysisDir | Out-Null
New-Item -ItemType Directory -Force -Path $rawDir | Out-Null
New-Item -ItemType Directory -Force -Path $logDir | Out-Null
New-Item -ItemType Directory -Force -Path $privateDir | Out-Null

function Release-Com($Value) {
    if ($null -ne $Value -and [System.Runtime.InteropServices.Marshal]::IsComObject($Value)) {
        try { [void][System.Runtime.InteropServices.Marshal]::FinalReleaseComObject($Value) } catch {}
    }
}

function Close-ComDocument($Document) {
    if ($null -eq $Document) { return }
    try { $Document.Saved = $true } catch {}
    try { $Document.Close() } catch {}
    Release-Com $Document
}

function Get-OracleTagValue {
    param([Parameter(Mandatory = $true)]$Shape)
    try {
        for ($i = 1; $i -le [int]$Shape.Tags.Count; $i++) {
            $tag = $null
            try {
                $tag = $Shape.Tags.Item($i)
                if ([string]$tag.Name -eq "PUB_ORACLE_ID") {
                    return [string]$tag.Value
                }
            }
            finally {
                Release-Com $tag
            }
        }
    }
    catch {
        return $null
    }
    return $null
}

function Find-TaggedShape {
    param(
        [Parameter(Mandatory = $true)]$Document,
        [Parameter(Mandatory = $true)][string]$TagValue
    )

    $foundPageIndex = 0
    $foundShapeIndex = 0
    $count = 0
    for ($pageIndex = 1; $pageIndex -le [int]$Document.Pages.Count; $pageIndex++) {
        $page = $null
        try {
            $page = $Document.Pages.Item($pageIndex)
            for ($shapeIndex = 1; $shapeIndex -le [int]$page.Shapes.Count; $shapeIndex++) {
                $shape = $null
                try {
                    $shape = $page.Shapes.Item($shapeIndex)
                    if ((Get-OracleTagValue -Shape $shape) -eq $TagValue) {
                        $count++
                        $foundPageIndex = $pageIndex
                        $foundShapeIndex = $shapeIndex
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

    if ($count -ne 1) {
        throw "Expected exactly one PUB_ORACLE_ID=$TagValue; found $count"
    }
    $page = $Document.Pages.Item($foundPageIndex)
    $shape = $page.Shapes.Item($foundShapeIndex)
    return [pscustomobject]@{
        page = $page
        shape = $shape
    }
}

function Convert-PublisherRgb {
    param([Parameter(Mandatory = $true)][long]$Value)
    $raw = [uint32]$Value
    return [ordered]@{
        packed = [long]$Value
        rgb = @(
            [int]($raw -band 0xFF),
            [int](($raw -shr 8) -band 0xFF),
            [int](($raw -shr 16) -band 0xFF)
        )
    }
}

function Get-SchemeSnapshot {
    param(
        [Parameter(Mandatory = $true)]$Document,
        [Parameter(Mandatory = $true)]$Shape,
        [Parameter(Mandatory = $true)][int]$SchemeIndex,
        [Parameter(Mandatory = $true)][string]$Phase
    )

    $scheme = $null
    $schemeColor = $null
    $fill = $null
    $foreColor = $null
    try {
        $scheme = $Document.ColorScheme
        $schemeColor = $scheme.Colors($SchemeIndex)
        $fill = $Shape.Fill
        $foreColor = $fill.ForeColor

        $shapeScheme = [int]$foreColor.SchemeColor
        $shapeRgbRaw = [long]$foreColor.RGB
        $schemeRgbRaw = [long]$schemeColor.RGB

        return [ordered]@{
            phase = $Phase
            requested_scheme_index = $SchemeIndex
            shape_id = [long]$Shape.ID
            shape_name = [string]$Shape.Name
            tag = Get-OracleTagValue -Shape $Shape
            shape_scheme_color = $shapeScheme
            shape_rgb = Convert-PublisherRgb $shapeRgbRaw
            document_scheme = [ordered]@{
                name = [string]$scheme.Name
                indexed_rgb = Convert-PublisherRgb $schemeRgbRaw
            }
        }
    }
    finally {
        Release-Com $foreColor
        Release-Com $fill
        Release-Com $schemeColor
        Release-Com $scheme
    }
}

function Test-RgbEqual {
    param($Left, $Right)
    if ($null -eq $Left -or $null -eq $Right) { return $false }
    $a = @($Left)
    $b = @($Right)
    if ($a.Count -ne 3 -or $b.Count -ne 3) { return $false }
    return ([int]$a[0] -eq [int]$b[0] -and [int]$a[1] -eq [int]$b[1] -and [int]$a[2] -eq [int]$b[2])
}

function Invoke-RawReceipt {
    param(
        [Parameter(Mandatory = $true)][string]$SourcePath,
        [Parameter(Mandatory = $true)][string]$ReceiptPath,
        [Parameter(Mandatory = $true)][string]$ReceiptExe
    )
    & $ReceiptExe $SourcePath $ReceiptPath
    if ($LASTEXITCODE -ne 0) {
        throw "scheme_color_oracle_receipt failed with exit code $LASTEXITCODE"
    }
    return Get-Content -LiteralPath $ReceiptPath -Raw | ConvertFrom-Json
}

function Build-RawReceiptTool {
    $cargo = Get-Command cargo -ErrorAction Stop
    if ($null -eq $cargo) {
        throw "cargo is required on the native Publisher runner for this bounded oracle"
    }

    Push-Location $repoRoot
    try {
        & cargo build --locked --release --manifest-path "vendor/producer-a/Cargo.toml" -p pub-reader --bin scheme_color_oracle_receipt
        if ($LASTEXITCODE -ne 0) {
            throw "raw oracle receipt build failed with exit code $LASTEXITCODE"
        }
    }
    finally {
        Pop-Location
    }

    $exe = Join-Path $repoRoot "vendor/producer-a/target/release/scheme_color_oracle_receipt.exe"
    if (-not (Test-Path -LiteralPath $exe)) {
        throw "raw oracle receipt executable missing after build"
    }
    return $exe
}

function Invoke-SchemeArm {
    param(
        [Parameter(Mandatory = $true)][int]$SchemeIndex,
        [Parameter(Mandatory = $true)][string]$ReceiptExe,
        [Parameter(Mandatory = $true)]$BaselineRaw
    )

    $expectedOrdinal = $SchemeIndex - 1
    $armName = "scheme-$SchemeIndex"
    $tagValue = "SCHEME_COLOR_$SchemeIndex"
    $armDir = Join-Path $privateDir $armName
    New-Item -ItemType Directory -Force -Path $armDir | Out-Null
    $inputPath = Join-Path $armDir "input.pub"
    $outputPath = Join-Path $armDir "output.pub"
    Copy-Item -LiteralPath $env:PUB_RESEARCH_FIXTURE -Destination $inputPath -Force

    $application = $null
    $document = $null
    $page = $null
    $shape = $null
    $before = $null
    try {
        $application = New-PubPublisherApplication
        $document = $application.Open($inputPath, $false, $false)
        if ([int]$document.Pages.Count -ne 1) {
            throw "Expected one-page blank fixture; found $([int]$document.Pages.Count)"
        }
        $page = $document.Pages.Item(1)
        if ([int]$page.Shapes.Count -ne 0) {
            throw "Expected zero Page.Shapes in blank fixture; found $([int]$page.Shapes.Count)"
        }

        $shape = $page.Shapes.AddShape($MsoShapeRectangle, 72, 72, 144, 72)
        $shape.Tags.Add("PUB_ORACLE_ID", $tagValue) | Out-Null
        try { $shape.Line.Visible = $MsoFalse } catch {}
        $shape.Fill.Solid()
        $shape.Fill.ForeColor.SchemeColor = $SchemeIndex

        $before = Get-SchemeSnapshot -Document $document -Shape $shape -SchemeIndex $SchemeIndex -Phase "before_save"
        $document.SaveAs($outputPath, $PbFilePublication, $false)
    }
    finally {
        Release-Com $shape
        Release-Com $page
        Close-ComDocument $document
        Close-PubPublisherApplication $application
    }

    if (-not (Test-Path -LiteralPath $outputPath)) {
        throw "$armName did not produce output.pub"
    }

    $reopenApplication = $null
    $reopenDocument = $null
    $reopenPage = $null
    $reopenShape = $null
    try {
        $reopenApplication = New-PubPublisherApplication
        $reopenDocument = $reopenApplication.Open($outputPath, $true, $false)
        $found = Find-TaggedShape -Document $reopenDocument -TagValue $tagValue
        $reopenPage = $found.page
        $reopenShape = $found.shape
        $after = Get-SchemeSnapshot -Document $reopenDocument -Shape $reopenShape -SchemeIndex $SchemeIndex -Phase "fresh_reopen"
    }
    finally {
        Release-Com $reopenShape
        Release-Com $reopenPage
        Close-ComDocument $reopenDocument
        Close-PubPublisherApplication $reopenApplication
    }

    $rawPath = Join-Path $rawDir "$armName.json"
    $raw = Invoke-RawReceipt -SourcePath $outputPath -ReceiptPath $rawPath -ReceiptExe $ReceiptExe

    if ([int]$raw.oplsccm.declared_count -ne 8) {
        throw "$armName OplSccm declared_count=$($raw.oplsccm.declared_count), expected 8"
    }
    if (@($raw.oplsccm.slots).Count -ne 8) {
        throw "$armName OplSccm slot count=$(@($raw.oplsccm.slots).Count), expected 8"
    }

    $slot = @($raw.oplsccm.slots)[$expectedOrdinal]
    $slotRgb = @($slot.rgb)
    $schemeRgb = @($after.document_scheme.indexed_rgb.rgb)
    $shapeRgb = @($after.shape_rgb.rgb)

    $anchoredSchemeFills = @(
        $raw.officeart.scheme_fills |
            Where-Object { [bool]$_.has_client_anchor -and [int]$_.scheme_ordinal -eq $expectedOrdinal }
    )
    $baselineAnchoredSameOrdinal = @(
        $BaselineRaw.officeart.scheme_fills |
            Where-Object { [bool]$_.has_client_anchor -and [int]$_.scheme_ordinal -eq $expectedOrdinal }
    )

    $checks = [ordered]@{
        com_scheme_index_before = ([int]$before.shape_scheme_color -eq $SchemeIndex)
        com_scheme_index_reopen = ([int]$after.shape_scheme_color -eq $SchemeIndex)
        com_shape_rgb_matches_document_scheme = (Test-RgbEqual $shapeRgb $schemeRgb)
        raw_slot_ordinal = ([int]$slot.ordinal -eq $expectedOrdinal)
        raw_slot_rgb_matches_document_scheme = (Test-RgbEqual $slotRgb $schemeRgb)
        anchored_fill_has_expected_raw_ordinal = ($anchoredSchemeFills.Count -ge 1)
        anchored_fill_expected_ordinal_is_new_vs_blank = ($anchoredSchemeFills.Count -gt $baselineAnchoredSameOrdinal.Count)
    }
    $passed = -not (@($checks.Values) -contains $false)

    return [ordered]@{
        scheme_index = $SchemeIndex
        expected_raw_ordinal = $expectedOrdinal
        tag = $tagValue
        publisher_before_save = $before
        publisher_fresh_reopen = $after
        raw = [ordered]@{
            source_sha256 = [string]$raw.source_sha256
            oplsccm_seq_num = [int]$raw.oplsccm.seq_num
            oplsccm_name = [string]$raw.oplsccm.name
            slot = $slot
            anchored_scheme_fill_matches = $anchoredSchemeFills
            blank_anchored_same_ordinal_count = $baselineAnchoredSameOrdinal.Count
        }
        checks = $checks
        passed = $passed
    }
}

$receiptExe = Build-RawReceiptTool
$baselineRawPath = Join-Path $rawDir "blank-fixture.json"
$baselineRaw = Invoke-RawReceipt -SourcePath $env:PUB_RESEARCH_FIXTURE -ReceiptPath $baselineRawPath -ReceiptExe $receiptExe

$arms = @()
for ($index = 1; $index -le 8; $index++) {
    $arms += Invoke-SchemeArm -SchemeIndex $index -ReceiptExe $receiptExe -BaselineRaw $baselineRaw
}

$allPassed = -not (@($arms | ForEach-Object { [bool]$_.passed }) -contains $false)
$ordinalMap = @()
foreach ($arm in $arms) {
    $ordinalMap += [ordered]@{
        publisher_scheme_index = [int]$arm.scheme_index
        raw_oplsccm_ordinal = [int]$arm.expected_raw_ordinal
        rgb = @($arm.publisher_fresh_reopen.document_scheme.indexed_rgb.rgb)
    }
}

$summary = [ordered]@{
    schema = "chaptera.viewer-scheme-color-publisher-oracle.v1"
    experiment_id = $ExpectedExperiment
    fixture = [ordered]@{
        sha256 = $fixtureHash
        blank_page_shapes = 0
    }
    publisher = [ordered]@{
        exe_sha256 = $publisherHash
        expected_version_prefix = "16.0.12527."
    }
    claim = [ordered]@{
        tested_scheme_indices = @(1, 2, 3, 4, 5, 6, 7, 8)
        expected_relation = "raw_oplsccm_ordinal = publisher_scheme_index - 1"
        all_eight_indices_passed = $allPassed
        ordinal_map = $ordinalMap
    }
    baseline_raw = [ordered]@{
        source_sha256 = [string]$baselineRaw.source_sha256
        oplsccm_declared_count = [int]$baselineRaw.oplsccm.declared_count
        oplsccm_name = [string]$baselineRaw.oplsccm.name
        anchored_scheme_fill_count = @(
            $baselineRaw.officeart.scheme_fills | Where-Object { [bool]$_.has_client_anchor }
        ).Count
    }
    arms = $arms
    evidence_boundary = [ordered]@{
        publisher_generated_pub_bytes_private = $true
        public_evidence_contains_only_hashes_spans_indices_and_rgb = $true
        source_fixture_immutable = $true
        libmspub_runtime_dependency = $false
        raw_receipt_uses_chaptera_pub_contents_and_pub_escher = $true
        statement = "Each arm starts from the same pinned zero-shape Publisher fixture, creates one tagged rectangle with Fill.ForeColor.SchemeColor=1..8, saves and fresh-reopens in pinned Publisher 2019, then joins COM SchemeColor/RGB to raw0x5C OplSccm slots and OfficeArt fSchemeIndex fillColor observations."
    }
}

if (-not $allPassed) {
    Write-PubJson -Value $summary -Path (Join-Path $analysisDir "scheme-color-oracle-01.json")
    throw "One or more scheme-color oracle arms failed bounded acceptance"
}

Write-PubJson -Value $summary -Path (Join-Path $analysisDir "scheme-color-oracle-01.json")

$logLines = @(
    "experiment=$ExpectedExperiment",
    "fixture_sha256=$fixtureHash",
    "publisher_exe_sha256=$publisherHash",
    "baseline_oplsccm_count=$($baselineRaw.oplsccm.declared_count)",
    "all_eight_indices_passed=$allPassed"
)
foreach ($arm in $arms) {
    $rgb = @($arm.publisher_fresh_reopen.document_scheme.indexed_rgb.rgb) -join ","
    $logLines += "scheme_index=$($arm.scheme_index) raw_ordinal=$($arm.expected_raw_ordinal) rgb=$rgb passed=$($arm.passed)"
}
$logLines | Set-Content -LiteralPath (Join-Path $logDir "scheme-color-oracle-01.txt") -Encoding ASCII
