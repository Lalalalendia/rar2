param([Parameter(Mandatory=$true)][string]$OutputRoot)
Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot "../..")).Path
Import-Module (Join-Path $RepoRoot "tools/windows/pub-runtime/PubRuntime.psm1") -Force
$SeedSha = "ffed034ac87e679f0bd08ff9cf74ad11c0e0e510a42b1bc1a7502415f6c29c87"
$SeedUrl = "https://raw.githubusercontent.com/apache/poi/942d95d85b15d0dfdb3bc9ba1b4f273f277757c8/test-data/publisher/SampleBrochure.pub"
$ExeSha = "e1ef8811b85b82045f37c4173b92726101be3a25e550b0dcb9f178df834ab20b"

function FileSha($p) { (Get-FileHash -LiteralPath $p -Algorithm SHA256).Hash.ToLowerInvariant() }
function AssertFile($p,$hash,[long]$size=-1) {
    if (-not (Test-Path -LiteralPath $p -PathType Leaf)) { throw "Pinned input missing" }
    if ((FileSha $p) -ne $hash) { throw "Pinned input SHA mismatch" }
    if ($size -ge 0 -and (Get-Item -LiteralPath $p).Length -ne $size) { throw "Pinned input size mismatch" }
}
function RunExe([string]$file,[string[]]$argv) {
    & $file @argv
    if ($LASTEXITCODE -ne 0) { throw "Native research executable returned an error" }
}
function RunCargo([string[]]$argv) {
    & cargo @argv
    if ($LASTEXITCODE -ne 0) { throw "Cargo research build failed" }
}
function ReleaseCom($obj) {
    if ($null -ne $obj -and [Runtime.InteropServices.Marshal]::IsComObject($obj)) {
        try { [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($obj) } catch {}
    }
}
function CloseDoc($doc) {
    if ($null -ne $doc) {
        try { $doc.Close() } catch {}
        ReleaseCom $doc
    }
}
function Hr($exception) {
    ("0x{0:X8}" -f ([long]$exception.HResult -band 0xFFFFFFFFL))
}
function TargetShape($doc,[long]$TargetId) {
    $snapshot = $null
    for ($i=1; $i -le [int]$doc.Pages.Count; $i++) {
        $page = $null
        try {
            $page = $doc.Pages.Item($i)
            for ($j=1; $j -le [int]$page.Shapes.Count; $j++) {
                $shape = $null
                try {
                    $shape = $page.Shapes.Item($j)
                    if ([long]$shape.ID -ne $TargetId) { continue }
                    if ($null -ne $snapshot) { throw "Duplicated COM target Shape.ID" }
                    $snapshot = [ordered]@{
                        page_index = $i
                        shape_id = [long]$shape.ID
                        left_emu = [long][math]::Round([double]$shape.Left * 12700)
                        top_emu = [long][math]::Round([double]$shape.Top * 12700)
                        width_emu = [long][math]::Round([double]$shape.Width * 12700)
                        height_emu = [long][math]::Round([double]$shape.Height * 12700)
                    }
                }
                finally { ReleaseCom $shape }
            }
        }
        finally { ReleaseCom $page }
    }
    if ($null -eq $snapshot) { throw "COM target Shape.ID not found" }
    $snapshot
}
# T352-PATHLEN-01: no geometry patching. Every input arm is an exact byte
# copy of the one-Save-normalized source, renamed to isolate UTF-16 path growth.
function NativeSaveAndReopen([string]$Source,[long]$TargetId) {
    $openGeometry = $null
    $app=$null; $doc=$null
    try {
        $app = New-PubPublisherApplication
        $doc = $app.Open($Source,$false,$false)
        $openGeometry = TargetShape $doc $TargetId
        $doc.Save()
    }
    finally {
        CloseDoc $doc
        Close-PubPublisherApplication $app
    }
    $reopenGeometry = $null
    $app2=$null; $doc2=$null
    try {
        $app2 = New-PubPublisherApplication
        $doc2 = $app2.Open($Source,$true,$false)
        $reopenGeometry = TargetShape $doc2 $TargetId
    }
    finally {
        CloseDoc $doc2
        Close-PubPublisherApplication $app2
    }
    [ordered]@{open="accepted";save="accepted";reopen="accepted";
        open_geometry=$openGeometry;reopen_geometry=$reopenGeometry}
}

function GeometryStable($a,$b) {
    foreach ($field in @("width_emu","height_emu","left_emu","top_emu")) {
        if ([math]::Abs([double]$a[$field] - [double]$b[$field]) -gt 5) { return $false }
    }
    return $true
}
function FingerprintStream($data,[string]$streamName) {
    $hits = @($data.streams | Where-Object { [string]$_.path -eq $streamName })
    if ($hits.Count -ne 1) { throw "Expected one /Contents stream fingerprint" }
    return $hits[0]
}

if (Test-Path -LiteralPath $OutputRoot) {
    if (@(Get-ChildItem -LiteralPath $OutputRoot -Force).Count -gt 0) {
        throw "T352-PATHLEN output root must be empty"
    }
}
$private = Join-Path $OutputRoot "private"
$analysis = Join-Path $OutputRoot "analysis"
$arms = Join-Path $private "arms"
New-Item -ItemType Directory -Force -Path $private,$analysis,$arms | Out-Null
$progress = Join-Path $analysis "t352-pathlen-progress.json"
Write-PubJson -Value ([ordered]@{
    schema="chaptera.t352-pathlen-progress.v1";stage="preflight";observed_arm_count=0
}) -Path $progress

$id = Get-PubPublisherIdentity
if (-not $id.available -or $id.version.state -ne "value" -or
    [string]$id.version.value -ne "16.0" -or $id.build.state -ne "value" -or
    [string]$id.build.value -ne "12527" -or $id.path.state -ne "value") {
    throw "Publisher2019 direct x86 COM identity gate failed"
}
$exe = Join-Path ([string]$id.path.value) "MSPUB.EXE"
AssertFile $exe $ExeSha
if ([string][Diagnostics.FileVersionInfo]::GetVersionInfo($exe).FileVersion -ne "16.0.12527.22145") {
    throw "Publisher2019 module version mismatch"
}

$seed = Join-Path $private "source.pub"
Invoke-WebRequest -Uri $SeedUrl -OutFile $seed -UseBasicParsing
AssertFile $seed $SeedSha 161792
$base = Join-Path $private "normalized.pub"
Copy-Item -LiteralPath $seed -Destination $base
$app0=$null; $doc0=$null
try {
    $app0 = New-PubPublisherApplication
    $doc0 = $app0.Open($base,$false,$false)
    $doc0.Save()
}
finally {
    CloseDoc $doc0
    Close-PubPublisherApplication $app0
}
AssertFile $seed $SeedSha 161792
$normalizedSha = FileSha $base

$manifest = Join-Path $RepoRoot "tools/pub-re/Cargo.toml"
RunCargo -argv @(
    "build","--release","--manifest-path",$manifest,
    "--bin","t352_crosswalk_census",
    "--bin","val_xproj_patch",
    "--bin","t352_pathlen_layout"
)
$metadata = cargo metadata --no-deps --format-version 1 --manifest-path $manifest | ConvertFrom-Json
if ($LASTEXITCODE -ne 0) { throw "Cargo metadata failed" }
$target = Join-Path ([string]$metadata.target_directory) "release"
$censusTool = Join-Path $target "t352_crosswalk_census.exe"
$fingerprintTool = Join-Path $target "val_xproj_patch.exe"
$layoutTool = Join-Path $target "t352_pathlen_layout.exe"
foreach ($path in @($censusTool,$fingerprintTool,$layoutTool)) {
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
        throw "Required T352-PATHLEN binary is missing"
    }
}

$censusPath = Join-Path $analysis "t352-pathlen-base-census.json"
RunExe -file $censusTool -argv @($base,$censusPath)
$baseCensus = Get-Content -LiteralPath $censusPath -Raw | ConvertFrom-Json
if ([string]$baseCensus.schema -ne "chaptera.t352-independent-identity-first-preflight.v1" -or
    [string]$baseCensus.source_sha256 -ne $normalizedSha -or
    [int]$baseCensus.identity_joined_pair_count -ne 30) {
    throw "Normalized Brochure identity-first 30-shape baseline mismatch"
}
$selected = @($baseCensus.rows | Where-Object {
    [long]$_.contents_seq -eq 305 -and [long]$_.spid -eq 1035 -and
    [int]$_.shape_type -eq 1 -and $_.unique_join -eq $true -and
    $_.both_geometries_consistent -eq $true
})
if ($selected.Count -ne 1) { throw "T352-PATHLEN native target is not uniquely verified" }
$baselineApp=$null; $baselineDoc=$null
try {
    $baselineApp = New-PubPublisherApplication
    $baselineDoc = $baselineApp.Open($base,$true,$false)
    $baselineCom = TargetShape $baselineDoc 305
}
finally {
    CloseDoc $baselineDoc
    Close-PubPublisherApplication $baselineApp
}
if ([math]::Abs([double]$baselineCom.width_emu - [double]$selected[0].contents_width_emu) -gt 5 -or
    [math]::Abs([double]$baselineCom.height_emu - [double]$selected[0].contents_height_emu) -gt 5) {
    throw "T352-PATHLEN COM baseline differs from identity-first native geometry"
}
AssertFile $base $normalizedSha
Write-PubJson -Value ([ordered]@{
    schema="chaptera.t352-pathlen-progress.v1";stage="normalized_and_joined"
    normalized_sha256=$normalizedSha;shape_id=305;spid=1035;baseline_joined_count=30
}) -Path $progress

$cases = @(
    [ordered]@{label="control";stem="aaaaa";expected=0},
    [ordered]@{label="same_length";stem="zzzzz";expected=0},
    [ordered]@{label="plus_two";stem="bbbbbb";expected=2},
    [ordered]@{label="plus_ten";stem="cccccccccc";expected=10}
)
$observations = @()
foreach ($case in $cases) {
    $label = [string]$case.label
    $leaf = ("{0}.pub" -f $case.stem)
    $arm = Join-Path $arms $leaf
    if (Test-Path -LiteralPath $arm) { throw "Refusing to reuse private native arm" }
    Copy-Item -LiteralPath $base -Destination $arm
    AssertFile $arm $normalizedSha

    $beforeCfb = Join-Path $analysis ("t352-pathlen-{0}-before-cfb.json" -f $label)
    $afterCfb = Join-Path $analysis ("t352-pathlen-{0}-after-cfb.json" -f $label)
    $beforeProjection = Join-Path $analysis ("t352-pathlen-{0}-before-projections.json" -f $label)
    $afterProjection = Join-Path $analysis ("t352-pathlen-{0}-after-projections.json" -f $label)
    $layoutPath = Join-Path $analysis ("t352-pathlen-{0}-layout.json" -f $label)

    RunExe -file $fingerprintTool -argv @("fingerprint",$arm,$beforeCfb)
    RunExe -file $censusTool -argv @($arm,$beforeProjection)
    $native = NativeSaveAndReopen -Source $arm -TargetId 305
    RunExe -file $fingerprintTool -argv @("fingerprint",$arm,$afterCfb)
    RunExe -file $censusTool -argv @($arm,$afterProjection)
    RunExe -file $layoutTool -argv @($base,$arm,"normalized.pub",$leaf,$layoutPath)

    $before = Get-Content -LiteralPath $beforeProjection -Raw | ConvertFrom-Json
    $after = Get-Content -LiteralPath $afterProjection -Raw | ConvertFrom-Json
    $beforeFingerprint = Get-Content -LiteralPath $beforeCfb -Raw | ConvertFrom-Json
    $afterFingerprint = Get-Content -LiteralPath $afterCfb -Raw | ConvertFrom-Json
    $layout = Get-Content -LiteralPath $layoutPath -Raw | ConvertFrom-Json
    $streamBefore = FingerprintStream $beforeFingerprint "/Contents"
    $streamAfter = FingerprintStream $afterFingerprint "/Contents"
    $delta = [long]$streamAfter.len - [long]$streamBefore.len

    if ([string]$before.source_sha256 -ne $normalizedSha -or
        [string]$beforeFingerprint.whole_file_sha256 -ne $normalizedSha -or
        [string]$layout.before_sha256 -ne $normalizedSha -or
        [string]$after.source_sha256 -ne [string]$afterFingerprint.whole_file_sha256 -or
        [string]$layout.after_sha256 -ne [string]$afterFingerprint.whole_file_sha256 -or
        [int]$before.identity_joined_pair_count -ne 30 -or
        [int]$after.identity_joined_pair_count -ne 30) {
        throw "T352-PATHLEN source identity / native 30-shape crosswalk mismatch"
    }
    if ([string]$layout.schema -ne "chaptera.t352-pathlen-layout-diff.v1" -or
        $delta -ne [long]$layout.observed_contents_len_delta -or
        [long]$layout.predicted_contents_len_delta -ne [long]$case.expected -or
        [int]$layout.before_slot_count -ne [int]$layout.after_slot_count) {
        throw "T352-PATHLEN structural receipt invalid or path topology has drifted"
    }
    $beforeRows = @($before.rows | Sort-Object { [long]$_.contents_seq })
    $afterRows = @($after.rows | Sort-Object { [long]$_.contents_seq })
    if ($beforeRows.Count -ne 30 -or $afterRows.Count -ne 30) {
        throw "T352-PATHLEN unexpected matched-shape row count"
    }
    $allScalarsStatic = $true
    $allContentsOffsetsRelocated = $true
    $allEscherOffsetsStatic = $true
    $fields = @(
        "contents_width_emu","contents_height_emu","anchor_width_emu","anchor_height_emu",
        "anchor_xs_emu","anchor_ys_emu","anchor_xe_emu","anchor_ye_emu"
    )
    for ($i=0; $i -lt 30; $i++) {
        $a = $beforeRows[$i]
        $b = $afterRows[$i]
        if ([long]$a.contents_seq -ne [long]$b.contents_seq -or
            [long]$a.spid -ne [long]$b.spid -or
            [int]$a.shape_type -ne [int]$b.shape_type -or
            $a.unique_join -ne $true -or $b.unique_join -ne $true) {
            throw "Identity-joined shape changed or lost on Save"
        }
        foreach ($field in $fields) {
            if ([long]$a.$field -ne [long]$b.$field) { $allScalarsStatic = $false }
        }
        foreach ($field in @("width_value_offset_in_contents","height_value_offset_in_contents")) {
            if (([long]$b.$field - [long]$a.$field) -ne $delta) {
                $allContentsOffsetsRelocated = $false
            }
        }
        foreach ($field in @("xe_tagged_field_offset_in_escher","ye_tagged_field_offset_in_escher")) {
            if ([long]$a.$field -ne [long]$b.$field) { $allEscherOffsetsStatic = $false }
        }
    }
    $comStatic = (GeometryStable $native.open_geometry $baselineCom) -and
                 (GeometryStable $native.reopen_geometry $baselineCom)
    $observations += [ordered]@{
        arm=$label
        filename_stem_length=[int]([string]$case.stem).Length
        open=$native.open
        save=$native.save
        reopen=$native.reopen
        predicted_contents_growth_bytes=[long]$case.expected
        observed_contents_growth_bytes=$delta
        path_length_matches=[bool]$layout.path_length_model_matches
        observed_trailer_pointer_delta=[long]$layout.trailer_offset_delta
        observed_directory_pointer_delta=[long]$layout.directory_offset_delta
        native_com_geometry_stable=$comStatic
        all_30_shapes_geometry_scalars_stable=$allScalarsStatic
        all_30_contents_offsets_shift_by_growth=$allContentsOffsetsRelocated
        all_30_escher_offsets_stable=$allEscherOffsetsStatic
        before_utf16_leaf_match_count=[int]$layout.before_utf16_leaf_match_count
        after_utf16_leaf_match_count=[int]$layout.after_utf16_leaf_match_count
        raw_leaf_localization_candidate=[bool]$layout.observed_leaf_localization
        byte_structure_receipt=("t352-pathlen-{0}-layout.json" -f $label)
    }
    Write-PubJson -Value ([ordered]@{
        schema="chaptera.t352-pathlen-progress.v1";stage="running";
        normalized_sha256=$normalizedSha;observed_arm_count=$observations.Count
    }) -Path $progress
}
AssertFile $seed $SeedSha 161792
AssertFile $base $normalizedSha
if ($observations.Count -ne 4) { throw "T352-PATHLEN experiment incomplete" }
$lawMatches = @($observations | Where-Object { -not $_.path_length_matches }).Count -eq 0
$validNoGeometry = @($observations | Where-Object {
    -not $_.native_com_geometry_stable -or -not $_.all_30_shapes_geometry_scalars_stable
}).Count -eq 0
$relocationMatches = @($observations | Where-Object {
    -not $_.all_30_contents_offsets_shift_by_growth
}).Count -eq 0
$hasByteLocalization = @($observations | Where-Object {
    -not $_.raw_leaf_localization_candidate
}).Count -eq 0
$verdict = if (-not $validNoGeometry) { "NOT_EVALUABLE_GEOMETRY_CHANGED" }
           elseif (-not $lawMatches) { "PATH_LENGTH_SIMPLE_MODEL_FALSIFIED" }
           elseif (-not $relocationMatches) { "LENGTH_MATCHES_OFFSETS_DIVERGE" }
           elseif ($hasByteLocalization) { "SUPPORTED_WITH_LEAF_BYTE_CANDIDATES" }
           else { "SUPPORTED_BOUNDED_BYTES_NOT_LOCALIZED" }
Write-PubJson -Value ([ordered]@{
    schema="chaptera.t352-native-observations.v1"
    axis="path_length"
    experiment_id="T352-PATHLEN-01"
    native_version="16.0.12527.22145"
    publisher_exe_sha256=$ExeSha
    seed_sha256=$SeedSha
    normalized_base_sha256=$normalizedSha
    baseline_joined_shape_count=30
    geometry_patch_count=0
    expected_arm_count=4
    observed_arm_count=$observations.Count
    source_safe_verdict=$verdict
    observations=$observations
    format_law_promoted=$false
}) -Path (Join-Path $analysis "t352-native-observations.json")
Write-PubJson -Value ([ordered]@{
    schema="chaptera.t352-pathlen-progress.v1";stage="completed";
    normalized_sha256=$normalizedSha;observed_arm_count=4;verdict=$verdict
}) -Path $progress
