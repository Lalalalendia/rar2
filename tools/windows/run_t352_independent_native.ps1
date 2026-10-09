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
function ObserveArm([string]$name,[string]$source,[string]$patcher,[string]$censusTool,[long]$TargetId,[string]$analysis) {
    $beforeSha = FileSha $source
    RunExe -file $patcher -argv @("fingerprint", $source, (Join-Path $analysis "$name-before-cfb.json"))
    RunExe -file $censusTool -argv @($source, (Join-Path $analysis "$name-before-projections.json"))

    $app = $null; $doc = $null
    $openSucceeded = $false
    $opened = $null; $openError = $null; $geometryError = $null; $saveError = $null
    try {
        try {
            $app = New-PubPublisherApplication
            $doc = $app.Open($source, $false, $false)
            $openSucceeded = $true
        }
        catch { $openError = Hr $_.Exception }
        if ($openSucceeded) {
            try { $opened = TargetShape $doc $TargetId }
            catch { $geometryError = Hr $_.Exception }
            try { $doc.Save() }
            catch { $saveError = Hr $_.Exception }
        }
    }
    finally {
        CloseDoc $doc
        Close-PubPublisherApplication $app
    }

    $afterSha = FileSha $source
    RunExe -file $patcher -argv @("fingerprint", $source, (Join-Path $analysis "$name-after-cfb.json"))
    $projectionState = "observed"
    try { RunExe -file $censusTool -argv @($source, (Join-Path $analysis "$name-after-projections.json")) }
    catch { $projectionState = "not_evaluable" }

    $reopened = $null; $reopenSucceeded = $false
    $reopenError = $null; $reopenGeometryError = $null
    if ($openSucceeded -and $null -eq $saveError) {
        $app2 = $null; $doc2 = $null
        try {
            try {
                $app2 = New-PubPublisherApplication
                $doc2 = $app2.Open($source, $true, $false)
                $reopenSucceeded = $true
            }
            catch { $reopenError = Hr $_.Exception }
            if ($reopenSucceeded) {
                try { $reopened = TargetShape $doc2 $TargetId }
                catch { $reopenGeometryError = Hr $_.Exception }
            }
        }
        finally {
            CloseDoc $doc2
            Close-PubPublisherApplication $app2
        }
    }
    [ordered]@{
        arm = $name
        initial_sha256 = $beforeSha
        after_save_sha256 = $afterSha
        open = if ($openSucceeded) { "accepted" } else { "failed" }
        open_hresult = $openError
        open_geometry = $opened
        open_geometry_hresult = $geometryError
        save = if (-not $openSucceeded) { "not_attempted" } elseif ($null -eq $saveError) { "accepted" } else { "failed" }
        save_hresult = $saveError
        reopen = if ($reopenSucceeded) { "accepted" } elseif (-not $openSucceeded -or $null -ne $saveError) { "not_attempted" } else { "failed" }
        reopen_hresult = $reopenError
        reopen_geometry = $reopened
        reopen_geometry_hresult = $reopenGeometryError
        post_save_projection_state = $projectionState
    }
}

if (Test-Path -LiteralPath $OutputRoot) {
    if (@(Get-ChildItem -LiteralPath $OutputRoot -Force).Count -gt 0) { throw "Output root must be empty" }
}
$analysis = Join-Path $OutputRoot "analysis"
$private = Join-Path $OutputRoot "private"
New-Item -ItemType Directory -Force -Path $analysis,$private | Out-Null
$progress = Join-Path $analysis "t352-progress.json"
Write-PubJson -Value ([ordered]@{schema="chaptera.t352-progress.v1"; stage="preflight"}) -Path $progress
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
    $doc0 = $app0.Open($base, $false, $false)
    $doc0.Save()
}
finally {
    CloseDoc $doc0
    Close-PubPublisherApplication $app0
}
AssertFile $seed $SeedSha 161792
$normalizedSha = FileSha $base
Write-PubJson -Value ([ordered]@{schema="chaptera.t352-progress.v1";stage="normalized";sha256=$normalizedSha}) -Path $progress

$vendor = Join-Path $RepoRoot "vendor/producer-a"
$vendorManifest = Join-Path $vendor "Cargo.toml"
$vendorLock = Join-Path $vendor "Cargo.lock"
$generatedLock = $false
try {
    if (-not (Test-Path -LiteralPath $vendorLock -PathType Leaf)) {
        RunCargo -argv @("generate-lockfile","--offline","--manifest-path",$vendorManifest)
        $generatedLock = $true
    }
    $vendorTarget = Join-Path $private "cargo-target"
    RunCargo -argv @("build","--release","--offline","--locked","--manifest-path",
        $vendorManifest,"--target-dir",$vendorTarget,"-p","pub-reader","--bin","t352_crosswalk_census")
}
finally {
    if ($generatedLock -and (Test-Path -LiteralPath $vendorLock -PathType Leaf)) {
        Remove-Item -LiteralPath $vendorLock -Force
    }
}
$tool = Join-Path $vendorTarget "release/t352_crosswalk_census.exe"
$census = Join-Path $private "independent-census.json"
RunExe -file $tool -argv @($base,$census)
$receipt = Get-Content -LiteralPath $census -Raw | ConvertFrom-Json
if ([string]$receipt.schema -ne "chaptera.t352-independent-identity-first-preflight.v1" -or
    [string]$receipt.source_sha256 -ne $normalizedSha) {
    throw "Independent normalized base/census SHA or schema mismatch"
}
$eligible = @($receipt.rows | Where-Object {
    $_.admitted_independent_target -eq $true -and [int]$_.shape_type -eq 1
} | Sort-Object { [long]$_.contents_seq })
if ($eligible.Count -lt 1) { throw "No unique type1 crosswalk on native-normalized independent fixture" }
$chosen = $eligible[0]
$targetId = [long]$chosen.contents_seq
$targetSpid = [long]$chosen.spid
$originalWidth = [long]$chosen.contents_width_emu
$changedWidth = $originalWidth + 127000
$checkApp=$null; $checkDoc=$null
try {
    $checkApp = New-PubPublisherApplication
    $checkDoc = $checkApp.Open($base,$true,$false)
    $baselineCom = TargetShape $checkDoc $targetId
    if ([math]::Abs([double]$baselineCom.width_emu - $originalWidth) -gt 5) {
        throw "Independent normalized COM baseline disagrees with exact raw width"
    }
}
finally {
    CloseDoc $checkDoc
    Close-PubPublisherApplication $checkApp
}
AssertFile $base $normalizedSha
Copy-Item -LiteralPath $census -Destination (Join-Path $analysis "t352-independent-census.json")
Write-PubJson -Value ([ordered]@{
    schema="chaptera.t352-progress.v1"; stage="independent-crosswalk";
    sha256=$normalizedSha; shape_id=$targetId; spid=$targetSpid; baseline_width_emu=$originalWidth
}) -Path $progress

$patchManifest = Join-Path $RepoRoot "tools/pub-re/Cargo.toml"
RunCargo -argv @("build","--release","--manifest-path",$patchManifest,"--bin","val_xproj_patch")
$patcherMetadata = cargo metadata --no-deps --format-version 1 --manifest-path $patchManifest | ConvertFrom-Json
$patcher = Join-Path ([string]$patcherMetadata.target_directory) "release\val_xproj_patch.exe"
if (-not (Test-Path -LiteralPath $patcher -PathType Leaf)) { throw "T352 patcher binary is missing" }
$arms = Join-Path $private "arms"
RunExe -file $patcher -argv @("prepare-independent",$base,$census,[string]$targetId,$arms)
Copy-Item -LiteralPath (Join-Path $arms "t352-independent-patch-receipt.json") -Destination (Join-Path $analysis "t352-independent-patch-receipt.json")
Write-PubJson -Value ([ordered]@{schema="chaptera.t352-progress.v1";stage="prepared";sha256=$normalizedSha}) -Path $progress

$observations = @()
foreach ($name in @("control","both_consistent","contents_only","escher_only")) {
    if ($observations.Count -eq 2 -and
        ($observations[0].reopen -ne "accepted" -or $null -eq $observations[0].reopen_geometry -or
         $observations[1].reopen -ne "accepted" -or $null -eq $observations[1].reopen_geometry)) {
        break
    }
    $arm = Join-Path $arms "$name.pub"
    $observations += (ObserveArm -name $name -source $arm -patcher $patcher -censusTool $tool -TargetId $targetId -analysis $analysis)
    Write-PubJson -Value ([ordered]@{
        schema = "chaptera.t352-native-observations.v1"
        experiment_id = "VAL-XPROJ-INDEPENDENT-01"
        native_version = "16.0.12527.22145"
        publisher_exe_sha256 = $ExeSha
        seed_sha256 = $SeedSha
        independent_fixture = "Apache-POI-SampleBrochure"
        target_shape_id = $targetId
        target_spid = $targetSpid
        baseline_width_emu = $originalWidth
        consistent_width_emu = $changedWidth
        normalized_base_sha256 = $normalizedSha
        observed_arm_count = $observations.Count
        observations = $observations
        open_api = "Publisher.Application.Open(FileName, ReadOnly, AddToRecentFiles)"
        prompt_for_bad_files = "unsupported_by_documented_Application_Open"
        semantic_law_status = "not_promoted"
    }) -Path (Join-Path $analysis "t352-native-observations.json")
}
AssertFile $seed $SeedSha 161792
AssertFile $base $normalizedSha
Write-PubJson -Value ([ordered]@{schema="chaptera.t352-progress.v1";stage="completed";count=$observations.Count}) -Path $progress
if ($observations.Count -ne 4) { throw "T352 independent matrix has fewer than four arms" }
foreach ($index in @(0,1)) {
    $arm = $observations[$index]
    if ($arm.open -ne "accepted" -or $arm.save -ne "accepted" -or $arm.reopen -ne "accepted" -or
        $null -eq $arm.open_geometry -or $null -eq $arm.reopen_geometry) {
        throw "Independent positive control failed; no read-authority law admitted"
    }
}
if ([math]::Abs([double]$observations[0].open_geometry.width_emu - $originalWidth) -gt 5 -or
    [math]::Abs([double]$observations[1].open_geometry.width_emu - $changedWidth) -gt 5) {
    throw "Independent positive control COM geometry mismatched"
}
