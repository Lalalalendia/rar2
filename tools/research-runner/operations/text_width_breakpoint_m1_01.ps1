param(
    [Parameter(Mandatory=$true)][string]$PacketPath,
    [Parameter(Mandatory=$true)][string]$OutputRoot
)
Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot "../../..")).Path
Import-Module (Join-Path $RepoRoot "tools/windows/pub-runtime/PubRuntime.psm1") -Force
$ExpectedPacket = "tools/research-runner/experiments/text-width-breakpoint-m1-01.packet.json"
$ExpectedId = "TEXT-WIDTH-BREAKPOINT-M1-01"
$PacketFull = Join-Path $RepoRoot $ExpectedPacket
$Worker = Join-Path $PSScriptRoot "text_width_breakpoint_m1_worker_01.ps1"
$PrivateDir = Join-Path $OutputRoot "private/text-width-m1"
$AnalysisDir = Join-Path $OutputRoot "analysis"
$LogDir = Join-Path $OutputRoot "logs"
$StagePath = Join-Path $AnalysisDir "text-width-m1-suite-stage.json"
New-Item -ItemType Directory -Force -Path $PrivateDir,$AnalysisDir,$LogDir | Out-Null

function Sha([string]$Path) {
    return (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
}
function Write-Stage([string]$State,[string]$Phase,[string]$Arm) {
    Write-PubJson -Path $StagePath -Value ([ordered]@{
        schema = "chaptera.text-width-m1-suite-stage.v1"
        experiment_id = $ExpectedId
        state = $State
        phase = $Phase
        arm_id = $Arm
    })
}
function Signature($Snapshot) {
    return (@($Snapshot.lines | ForEach-Object {
        "$([int]$_.start):$([int]$_.end)"
    }) -join "|")
}
function Run-Child([string]$Mode,[string]$InputFile,[string]$InputSha,[string]$Case,[double]$Width) {
    $ps = Join-Path $env:SystemRoot "System32/WindowsPowerShell/v1.0/powershell.exe"
    if (-not (Test-Path -LiteralPath $ps -PathType Leaf)) {
        throw "m1_windows_powershell_unavailable"
    }
    $quoted = @($Worker,$InputFile,$OutputRoot) | ForEach-Object {
        '"' + ($_ -replace '"','') + '"'
    }
    $logBase = Join-Path $PrivateDir ("child-" + $Case)
    $args = @("-NoProfile","-ExecutionPolicy","Bypass",
        "-File",$quoted[0],"-Mode",$Mode,"-SourcePath",$quoted[1],
        "-OutputRoot",$quoted[2],"-ArmId",$Case,
        "-WidthPt",([string]$Width),
        "-ExpectedSourceSha",$InputSha)
    $started = [DateTime]::UtcNow
    $child = Start-Process -FilePath $ps -ArgumentList $args -RedirectStandardOutput ($logBase + ".stdout.txt") -RedirectStandardError ($logBase + ".stderr.txt") -PassThru
    if (-not $child.WaitForExit(180000)) {
        if (-not $child.HasExited) {
            try { Stop-Process -Id $child.Id -Force -ErrorAction Stop }
            catch { throw "m1_child_ownership_cleanup_failed" }
            [void]$child.WaitForExit(10000)
        }
        $owned = @(Get-Process -Name MSPUB -ErrorAction SilentlyContinue | Where-Object {
            try { $_.StartTime.ToUniversalTime() -ge $started.AddSeconds(-2) }
            catch { $false }
        })
        if ($owned.Count -eq 1) {
            try { Stop-Process -Id $owned[0].Id -Force -ErrorAction Stop }
            catch { throw "m1_publisher_cleanup_failed" }
        } else {
            throw "m1_publisher_ownership_uncertain"
        }
        throw "m1_child_timeout"
    }
    $child.WaitForExit()
    $child.Refresh()
    if ([int]$child.ExitCode -ne 0) { throw "m1_child_invalid_$Case" }
    Start-Sleep -Seconds 2
    if (@(Get-Process -Name MSPUB -ErrorAction SilentlyContinue).Count -gt 0) {
        throw "m1_publisher_still_running_$Case"
    }
}

$CurrentPhase = "preflight"
$CurrentArm = "none"
try {
    Write-Stage "running" $CurrentPhase $CurrentArm
    if ((Resolve-Path -LiteralPath $PacketPath).Path -ne (Resolve-Path -LiteralPath $PacketFull).Path) {
        throw "m1_packet_not_allowlisted"
    }
    $packet = Get-Content -LiteralPath $PacketFull -Raw | ConvertFrom-Json
    if ($packet.id -ne $ExpectedId -or
        [string]$packet.publisher_environment -ne "publisher-2019" -or
        [string]$packet.operation.script -ne "tools/research-runner/operations/text_width_breakpoint_m1_01.ps1") {
        throw "m1_packet_identity_invalid"
    }
    if ($env:GITHUB_REF -ne "refs/heads/main" -or
        [string]::IsNullOrWhiteSpace($env:GITHUB_SHA)) {
        throw "m1_requires_trusted_main"
    }
    $actualCommit = (& git -C $RepoRoot rev-parse HEAD).Trim()
    if ($LASTEXITCODE -ne 0 -or $actualCommit -ne $env:GITHUB_SHA) {
        throw "m1_checkout_not_trusted"
    }
    & python (Join-Path $RepoRoot "tools/research-runner/validate_packet.py") --packet $ExpectedPacket --expected-environment publisher-2019 | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "m1_packet_contract_failed" }
    if (@(Get-Process -Name MSPUB -ErrorAction SilentlyContinue).Count -ne 0) {
        throw "m1_publisher_busy_before_run"
    }

    $CurrentPhase = "prepare"
    Write-Stage "running" $CurrentPhase $CurrentArm
    & (Join-Path $RepoRoot "tools/research-runner/prepare_native_run.ps1") -PacketPath $PacketFull -OutputRoot $OutputRoot
    $environment = Get-Content -LiteralPath (Join-Path $OutputRoot "environment.json") -Raw | ConvertFrom-Json

    $registry = Get-Content -LiteralPath (Join-Path $RepoRoot "tools/pub-re/native-fixtures.json") -Raw | ConvertFrom-Json
    if ($registry.schema -ne "chaptera.pub-re-native-fixture-registry.v1") {
        throw "m1_fixture_registry_invalid"
    }
    $matches = @($registry.fixtures | Where-Object { $_.alias -eq "sample-newsletter" })
    if ($matches.Count -ne 1) { throw "m1_fixture_alias_ambiguous" }
    $fixture = $matches[0]
    if ([string]$fixture.expected_sha256 -ne "6a825ba26ba35d6e885acdc62e859591ed37cb0ff7480b554b9cb362b644dfcf" -or
        [int64]$fixture.expected_byte_len -ne 291840) {
        throw "m1_fixture_not_allowlisted"
    }
    $uri = [Uri]([string]$fixture.download_url)
    if ($uri.Scheme -ne "https" -or $uri.Host -ne "raw.githubusercontent.com") {
        throw "m1_fixture_remote_untrusted"
    }
    $original = Join-Path $PrivateDir "source.pub"
    Invoke-WebRequest -Uri $uri.AbsoluteUri -OutFile $original -UseBasicParsing
    if ((Sha $original) -ne [string]$fixture.expected_sha256 -or
        (Get-Item -LiteralPath $original).Length -ne 291840) {
        throw "m1_fixture_hash_mismatch"
    }
    $fontFile = Join-Path $env:WINDIR "Fonts/arial.ttf"
    if (-not (Test-Path -LiteralPath $fontFile -PathType Leaf)) {
        throw "m1_physical_arial_missing"
    }
    $fontSha = Sha $fontFile

    $CurrentPhase = "seed"
    $CurrentArm = "none"
    Write-Stage "running" $CurrentPhase $CurrentArm
    Run-Child "seed" $original ([string]$fixture.expected_sha256) "none" 160.0
    $seedInfoPath = Join-Path $AnalysisDir "text-width-m1-seed.json"
    $seed = Get-Content -LiteralPath $seedInfoPath -Raw | ConvertFrom-Json
    $seedPath = Join-Path $PrivateDir "seed.pub"
    if ($seed.schema -ne "chaptera.text-width-m1-seed.v1" -or
        (Sha $seedPath) -ne [string]$seed.seed_sha256) {
        throw "m1_seed_receipt_invalid"
    }
    $cases = @(
        [pscustomobject]@{ Id="control-before"; Width=160.0 },
        [pscustomobject]@{ Id="narrow"; Width=148.0 },
        [pscustomobject]@{ Id="wide"; Width=172.0 },
        [pscustomobject]@{ Id="control-after"; Width=160.0 }
    )
    $arms = @()
    foreach ($case in $cases) {
        $CurrentPhase = "arm"
        $CurrentArm = [string]$case.Id
        Write-Stage "running" $CurrentPhase $CurrentArm
        Run-Child "arm" $seedPath ([string]$seed.seed_sha256) $CurrentArm ([double]$case.Width)
        $receiptPath = Join-Path $AnalysisDir ("text-width-m1-" + $CurrentArm + ".json")
        $receipt = Get-Content -LiteralPath $receiptPath -Raw | ConvertFrom-Json
        if ($receipt.schema -ne "chaptera.text-width-m1-arm.v1" -or
            [string]$receipt.arm_id -ne $CurrentArm -or
            [string]$receipt.seed_sha256 -ne [string]$seed.seed_sha256 -or
            -not [bool]$receipt.original_seed_preserved) {
            throw "m1_arm_receipt_invalid"
        }
        $arms += $receipt
    }

    $CurrentPhase = "comparison"
    $CurrentArm = "none"
    Write-Stage "running" $CurrentPhase $CurrentArm
    $signatures = @()
    foreach ($arm in $arms) {
        if ((Signature $arm.after) -ne (Signature $arm.fresh_reopen) -or
            [math]::Abs([double]$arm.after.width_pt - [double]$arm.fresh_reopen.width_pt) -gt 0.001 -or
            [string]$arm.after.text_utf16le_sha256 -ne [string]$arm.fresh_reopen.text_utf16le_sha256) {
            throw "m1_after_save_reopen_drift"
        }
        $signatures += [string](Signature $arm.fresh_reopen)
    }
    if ($signatures[0] -ne $signatures[3]) {
        throw "m1_bracketing_controls_disagree"
    }
    if ((Sha $original) -ne [string]$fixture.expected_sha256 -or
        (Sha $seedPath) -ne [string]$seed.seed_sha256) {
        throw "m1_source_or_seed_modified"
    }
    $effect = $signatures[1] -ne $signatures[2]
    $verdict = if ($effect) { "width_sensitivity_observed_no_threshold_law" } else { "inconclusive_no_break_contrast" }
    $summary = [ordered]@{
        schema = "chaptera.text-width-breakpoint-m1.v1"
        experiment_id = $ExpectedId
        repo_sha = $actualCommit
        packet_sha256 = Sha $PacketFull
        worker_sha256 = Sha $Worker
        original_source_sha256 = [string]$fixture.expected_sha256
        seed_sha256 = [string]$seed.seed_sha256
        publisher_exe_sha256 = [string]$environment.publisher.sha256
        font_family = "Arial"
        font_file_sha256 = $fontSha
        width_values_pt = @(160.0,148.0,172.0,160.0)
        arm_count = 4
        controls_stable = $true
        save_reopen_stable = $true
        contrast_seen = $effect
        status = $verdict
        line_counts = @($arms | ForEach-Object { [int]$_.fresh_reopen.line_count })
        break_signatures = $signatures
        source_bytes_uploaded = $false
        carrier_authority_granted = $false
        product_visual_acceptance_granted = $false
    }
    Write-PubJson -Value $summary -Path (Join-Path $AnalysisDir "text-width-breakpoint-m1-01.json")
    @("experiment=$ExpectedId","arms=4","status=$verdict","controls_stable=true","raw_pub_uploaded=false") |
        Set-Content -LiteralPath (Join-Path $LogDir "text-width-breakpoint-m1-01.txt") -Encoding ASCII

    $CurrentPhase = "finalize"
    Write-Stage "running" $CurrentPhase $CurrentArm
    & (Join-Path $RepoRoot "tools/research-runner/finalize_native_run.ps1") -PacketPath $PacketFull -OutputRoot $OutputRoot
    Write-Stage "complete" "complete" "none"
} catch {
    Write-Stage "invalid" $CurrentPhase $CurrentArm
    throw
}
