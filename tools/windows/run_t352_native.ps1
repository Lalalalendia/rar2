param([Parameter(Mandatory=$true)][string]$OutputRoot,[switch]$SetterFollowup)
Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot "../..")).Path
Import-Module (Join-Path $RepoRoot "tools/windows/pub-runtime/PubRuntime.psm1") -Force
$SeedSha = "6fefdef46b87c767150878dc384549cb2d2ec2ac54de25f8ddb3a5628301107e"
$SeedUrl = "https://raw.githubusercontent.com/apache/poi/942d95d85b15d0dfdb3bc9ba1b4f273f277757c8/test-data/publisher/Sample.pub"
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
function Shape293($doc) {
    $snapshot = $null
    for ($i=1; $i -le [int]$doc.Pages.Count; $i++) {
        $page = $null
        try {
            $page = $doc.Pages.Item($i)
            for ($j=1; $j -le [int]$page.Shapes.Count; $j++) {
                $shape = $null
                try {
                    $shape = $page.Shapes.Item($j)
                    if ([long]$shape.ID -ne 293) { continue }
                    if ($null -ne $snapshot) { throw "Duplicated COM Shape.ID=293" }
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
    if ($null -eq $snapshot) { throw "COM Shape.ID=293 not found" }
    $snapshot
}
function SetShape293Width($doc,[double]$targetPoints) {
    $count = 0
    for ($i=1; $i -le [int]$doc.Pages.Count; $i++) {
        $page=$null
        try {
            $page=$doc.Pages.Item($i)
            for ($j=1; $j -le [int]$page.Shapes.Count; $j++) {
                $shape=$null
                try {
                    $shape=$page.Shapes.Item($j)
                    if ([long]$shape.ID -ne 293) { continue }
                    $count++
                    if ($count -ne 1) { throw "Duplicate target COM Shape.ID293" }
                    $shape.Width=$targetPoints
                }
                finally { ReleaseCom $shape }
            }
        }
        finally { ReleaseCom $page }
    }
    if ($count -ne 1) { throw "Target Shape.ID293 missing during COM setter" }
}
function ObserveArm([string]$name,[string]$source,[string]$patcher,[string]$analysis,[string]$SetterMode="none") {
    if ($SetterMode -notin @("none","same_value","to_contents")) { throw "Unsupported setter mode" }
    $stem = if ($SetterMode -eq "none") { $name } else { "t352-$name" }
    $beforeSha = FileSha $source
    RunExe -file $patcher -argv @("fingerprint", $source, (Join-Path $analysis "$stem-before-cfb.json"))
    $beforeProjectionPath=Join-Path $analysis "$stem-before-projections.json"
    RunExe -file $patcher -argv @("inspect", $source, $beforeProjectionPath)
    $beforeProjection=Get-Content -LiteralPath $beforeProjectionPath -Raw | ConvertFrom-Json

    $app = $null; $doc = $null
    $openSucceeded = $false
    $opened = $null; $openError = $null; $geometryError = $null; $saveError = $null
    $setterError=$null; $setterTargetEmu=$null; $afterSetterGeometry=$null
    try {
        try {
            $app = New-PubPublisherApplication
            $doc = $app.Open($source, $false, $false)
            $openSucceeded = $true
        }
        catch { $openError = Hr $_.Exception }
        if ($openSucceeded) {
            try { $opened = Shape293 $doc }
            catch { $geometryError = Hr $_.Exception }
            if ($SetterMode -ne "none") {
                if ($null -ne $geometryError) { $setterError = "pre_set_geometry_failed" }
                else {
                    try {
                        $setterTargetEmu = if ($SetterMode -eq "same_value") {
                            [long]$opened.width_emu
                        } else {
                            [long]$beforeProjection.contents_width_emu
                        }
                        if ($setterTargetEmu -lt 50000 -or $setterTargetEmu -gt 10000000) {
                            throw "Setter target width outside bounded EMU range"
                        }
                        SetShape293Width $doc ([double]$setterTargetEmu / 12700.0)
                        $afterSetterGeometry = Shape293 $doc
                    }
                    catch { $setterError = Hr $_.Exception }
                }
            }
            if ($null -eq $setterError) {
                try { $doc.Save() }
                catch { $saveError = Hr $_.Exception }
            }
            else { $saveError = "setter_failed" }
        }
    }
    finally {
        CloseDoc $doc
        Close-PubPublisherApplication $app
    }

    $afterSha = FileSha $source
    RunExe -file $patcher -argv @("fingerprint", $source, (Join-Path $analysis "$stem-after-cfb.json"))
    $projectionState = "observed"
    try { RunExe -file $patcher -argv @("inspect", $source, (Join-Path $analysis "$stem-after-projections.json")) }
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
                try { $reopened = Shape293 $doc2 }
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
        setter_mode = $SetterMode
        setter_target_emu = $setterTargetEmu
        setter_hresult = $setterError
        after_setter_geometry = $afterSetterGeometry
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
AssertFile $seed $SeedSha 72192
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
AssertFile $seed $SeedSha 72192
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
        $vendorManifest,"--target-dir",$vendorTarget,"-p","pub-reader","--bin","structural_base_manifest")
}
finally {
    if ($generatedLock -and (Test-Path -LiteralPath $vendorLock -PathType Leaf)) {
        Remove-Item -LiteralPath $vendorLock -Force
    }
}
$tool = Join-Path $vendorTarget "release/structural_base_manifest.exe"
$structural = Join-Path $private "structural.json"
RunExe -file $tool -argv @($base, $structural)
$receipt = Get-Content -LiteralPath $structural -Raw | ConvertFrom-Json
if ($receipt.schema -ne "chaptera.modern-structural-base/v1" -or
    [string]$receipt.source_sha256 -ne $normalizedSha -or
    [int]$receipt.stream_count -ne 10 -or
    [int]$receipt.candidate_count -ne 6 -or
    [long]$receipt.selected_target.contents_seq_num -ne 293 -or
    [long]$receipt.selected_target.officeart_spid -ne 1025 -or
    -not [bool]$receipt.selected_target.contents_anchor_extent_exact) {
    throw "Fresh normalized base does not meet T370 semantic contract"
}

$patchManifest = Join-Path $RepoRoot "tools/pub-re/Cargo.toml"
RunCargo -argv @("build","--release","--manifest-path",$patchManifest,"--bin","val_xproj_patch")
$patcherMetadata = cargo metadata --no-deps --format-version 1 --manifest-path $patchManifest | ConvertFrom-Json
$patcher = Join-Path ([string]$patcherMetadata.target_directory) "release\val_xproj_patch.exe"
if (-not (Test-Path -LiteralPath $patcher -PathType Leaf)) { throw "T352 patcher executable missing from Cargo target directory" }
$arms = Join-Path $private "arms"
RunExe -file $patcher -argv @("prepare",$base,$structural,$arms)
Copy-Item -LiteralPath (Join-Path $arms "t352-patch-receipt.json") -Destination (Join-Path $analysis "t352-patch-receipt.json")
$setterVariants = @()
if ($SetterFollowup) {
    $setterVariants = @(
        [ordered]@{name="settercontentsame"; source="contents_only"; mode="same_value"}
        [ordered]@{name="settercontentstocontents"; source="contents_only"; mode="to_contents"}
        [ordered]@{name="settereschersame"; source="escher_only"; mode="same_value"}
        [ordered]@{name="settereschertocontents"; source="escher_only"; mode="to_contents"}
    )
    foreach ($variant in $setterVariants) {
        Copy-Item -LiteralPath (Join-Path $arms "$($variant.source).pub") 
            -Destination (Join-Path $arms "$($variant.name).pub")
    }
}
Write-PubJson -Value ([ordered]@{schema="chaptera.t352-progress.v1";stage="prepared";sha256=$normalizedSha}) -Path $progress

$observations = @()
foreach ($name in @("control","both_consistent","contents_only","escher_only")) {
    if ($observations.Count -eq 2 -and
        ($observations[0].reopen -ne "accepted" -or $null -eq $observations[0].reopen_geometry -or
         $observations[1].reopen -ne "accepted" -or $null -eq $observations[1].reopen_geometry)) {
        break
    }
    $arm = Join-Path $arms "$name.pub"
    $observations += (ObserveArm -name $name -source $arm -patcher $patcher -analysis $analysis)
    Write-PubJson -Value ([ordered]@{
        schema = "chaptera.t352-native-observations.v1"
        experiment_id = "VAL-XPROJ-01"
        native_version = "16.0.12527.22145"
        publisher_exe_sha256 = $ExeSha
        seed_sha256 = $SeedSha
        normalized_base_sha256 = $normalizedSha
        observed_arm_count = $observations.Count
        observations = $observations
        open_api = "Publisher.Application.Open(FileName, ReadOnly, AddToRecentFiles)"
        prompt_for_bad_files = "unsupported_by_documented_Application_Open"
        semantic_law_status = "not_promoted"
    }) -Path (Join-Path $analysis "t352-native-observations.json")
}
if ($observations.Count -ne 4) { throw "T352 controls failed; conflict arms were not all executed" }
$setterObservations = @()
if ($SetterFollowup) {
    if (@($observations | Where-Object { $_.open -ne "accepted" -or $_.save -ne "accepted" -or
            $_.reopen -ne "accepted" -or $null -eq $_.open_geometry -or
            $null -eq $_.reopen_geometry }).Count -gt 0) {
        throw "T352 primary controls failed; refusing setter followup"
    }
    foreach ($variant in $setterVariants) {
        $armPath=Join-Path $arms "$($variant.name).pub"
        $setterObservations += (ObserveArm -name $variant.name -source $armPath 
            -patcher $patcher -analysis $analysis -SetterMode $variant.mode)
        Write-PubJson -Value ([ordered]@{
            schema="chaptera.t352-setter-transitions.v1"
            experiment_id="VAL-XPROJ-SETTER-01"
            publisher_exe_sha256=$ExeSha
            normalized_base_sha256=$normalizedSha
            count=$setterObservations.Count
            observations=$setterObservations
            interpretation="unclassified_natively_observed_only"
        }) -Path (Join-Path $analysis "t352-setter-observations.json")
        Write-PubJson -Value ([ordered]@{
            schema="chaptera.t352-progress.v1";stage="setter";count=$setterObservations.Count
        }) -Path $progress
    }
    if ($setterObservations.Count -ne 4 -or
        @($setterObservations | Where-Object { $_.setter_hresult -ne $null -or
            $_.save -ne "accepted" -or $_.reopen -ne "accepted" }).Count -gt 0) {
        throw "T352 setter matrix failed; retained partial observations"
    }
}
AssertFile $seed $SeedSha 72192
AssertFile $base $normalizedSha
Write-PubJson -Value ([ordered]@{
    schema="chaptera.t352-progress.v1";stage="completed";count=$observations.Count;
    setter_count=$setterObservations.Count
}) -Path $progress
