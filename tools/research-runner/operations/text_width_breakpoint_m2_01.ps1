param(
    [Parameter(Mandatory=$true)][string]$PacketPath,
    [Parameter(Mandatory=$true)][string]$OutputRoot
)
Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot "../../..")).Path
Import-Module (Join-Path $RepoRoot "tools/windows/pub-runtime/PubRuntime.psm1") -Force
$ExpectedPacket = "tools/research-runner/experiments/text-width-breakpoint-m2-01.packet.json"
$ExpectedId = "TEXT-WIDTH-BREAKPOINT-M2-01"
$PacketFull = Join-Path $RepoRoot $ExpectedPacket
$Worker = Join-Path $PSScriptRoot "text_width_breakpoint_m1_worker_01.ps1"
$PrivateDir = Join-Path $OutputRoot "private/text-width-m1"
$AnalysisDir = Join-Path $OutputRoot "analysis"
$LogDir = Join-Path $OutputRoot "logs"
$StagePath = Join-Path $AnalysisDir "text-width-m2-suite-stage.json"
New-Item -ItemType Directory -Force -Path $PrivateDir,$AnalysisDir,$LogDir | Out-Null

function Sha([string]$Path) {
    return (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
}
function Write-Stage([string]$State,[string]$Phase,[string]$Arm) {
    Write-PubJson -Path $StagePath -Value ([ordered]@{
        schema = "chaptera.text-width-m2-suite-stage.v1"
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
        [string]$packet.operation.script -ne "tools/research-runner/operations/text_width_breakpoint_m2_01.ps1") {
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
    # M1 exact-head baseline proved that 160pt and 172pt have the same
    # first four breaks, but line five ends at UTF16 offsets 139 and 140.
    # M2 isolates that one boundary without granting a general format law.
    if ($fontSha -cne "c9b76220a5be42ead4733611e417cd65c5fd8aeaa33eb56576ac378a37d130a1") {
        throw "m2_physical_font_baseline_drift"
    }
    function Invoke-M2Arm([string]$Name, [double]$Width) {
        $script:CurrentPhase = "arm"
        $script:CurrentArm = $Name
        Write-Stage "running" $script:CurrentPhase $script:CurrentArm
        Run-Child "arm" $seedPath ([string]$seed.seed_sha256) $Name $Width
        $receiptPath = Join-Path $AnalysisDir ("text-width-m1-" + $Name + ".json")
        $receipt = Get-Content -LiteralPath $receiptPath -Raw | ConvertFrom-Json
        if ([string]$receipt.schema -cne "chaptera.text-width-m1-arm.v1" -or
            [string]$receipt.arm_id -cne $Name -or
            [math]::Abs([double]$receipt.requested_width_pt - $Width) -gt 0.001 -or
            [string]$receipt.seed_sha256 -cne [string]$seed.seed_sha256 -or
            -not [bool]$receipt.original_seed_preserved) {
            throw "m2_arm_receipt_invalid"
        }
        $after = $receipt.after
        $fresh = $receipt.fresh_reopen
        if ((Signature $after) -cne (Signature $fresh) -or
            [math]::Abs([double]$after.width_pt - [double]$fresh.width_pt) -gt 0.001 -or
            [string]$after.text_utf16le_sha256 -cne [string]$fresh.text_utf16le_sha256 -or
            -not [bool]$fresh.visible_font_name_matches -or
            [math]::Abs([double]$fresh.visible_font_size_pt - 12.0) -gt 0.001 -or
            -not [bool]$fresh.visible_text_matches_expected -or
            [int]$fresh.auto_fit_mode -ne 0) {
            throw "m2_after_save_reopen_or_font_drift"
        }
        return $receipt
    }
    function Assert-M2Line([object]$Snapshot) {
        if ([int]$Snapshot.line_count -ne 6) { throw "m2_line_count_changed" }
        $expectedEarly = @(28,57,86,112)
        $expectedStarts = @(0,28,57,86)
        for ($i=0; $i -lt 4; $i++) {
            if ([int]$Snapshot.lines[$i].start -ne $expectedStarts[$i] -or
                [int]$Snapshot.lines[$i].end -ne $expectedEarly[$i]) {
                throw "m2_earlier_line_break_changed"
            }
        }
        if ([int]$Snapshot.lines[4].start -ne 112 -or
            [int]$Snapshot.lines[4].end -notin @(139,140) -or
            [int]$Snapshot.lines[5].start -ne [int]$Snapshot.lines[4].end -or
            [int]$Snapshot.lines[5].end -ne 155) {
            throw "m2_nonbinary_terminal_layout"
        }
    }

    $script:CurrentPhase = "control_before"
    $script:CurrentArm = "control-before"
    $controlBefore = Invoke-M2Arm "control-before" 160.0
    $controlSig = [string](Signature $controlBefore.fresh_reopen)
    if ($controlSig -cne "0:28|28:57|57:86|86:112|112:139|139:155") {
        throw "m2_exact_m1_control_not_reproduced"
    }
    Assert-M2Line $controlBefore.fresh_reopen

    $highArm = Invoke-M2Arm "wide" 172.0
    Assert-M2Line $highArm.fresh_reopen
    if ((Signature $highArm.fresh_reopen) -cne "0:28|28:57|57:86|86:112|112:140|140:155") {
        throw "m2_exact_m1_positive_not_reproduced"
    }

    $arms = @($controlBefore, $highArm)
    $trace = @()
    [double]$low = 160.0
    [double]$high = 172.0
    for ($index=1; $index -le 7; $index++) {
        [double]$mid = ($low + $high) / 2.0
        $arm = Invoke-M2Arm ("m2-mid-" + $index) $mid
        Assert-M2Line $arm.fresh_reopen
        $arms += $arm
        $fifthEnd = [int]$arm.fresh_reopen.lines[4].end
        if ($fifthEnd -eq 139) {
            $low = $mid
        } elseif ($fifthEnd -eq 140) {
            $high = $mid
        } else {
            throw "m2_fifth_line_outside_dichotomy"
        }
        $trace += [ordered]@{
            midpoint_index = $index
            tested_width_pt = $mid
            fifth_line_end_utf16 = $fifthEnd
            lower_width_pt_after = $low
            upper_width_pt_after = $high
            break_signature = [string](Signature $arm.fresh_reopen)
        }
    }

    $controlAfter = Invoke-M2Arm "control-after" 160.0
    Assert-M2Line $controlAfter.fresh_reopen
    if ((Signature $controlAfter.fresh_reopen) -cne $controlSig) {
        throw "m2_bracketing_controls_disagree"
    }
    $arms += $controlAfter
    if ($arms.Count -ne 10 -or
        [math]::Abs(($high - $low) - 0.09375) -gt 0.000001) {
        throw "m2_bisection_budget_or_resolution_mismatch"
    }
    if ((Sha $original) -cne [string]$fixture.expected_sha256 -or
        (Sha $seedPath) -cne [string]$seed.seed_sha256) {
        throw "m2_source_or_seed_modified"
    }

    $script:CurrentPhase = "comparison"
    $script:CurrentArm = "none"
    Write-Stage "running" $script:CurrentPhase $script:CurrentArm
    $summary = [ordered]@{
        schema = "chaptera.text-width-breakpoint-m2.v1"
        experiment_id = $ExpectedId
        reuse_worker_protocol = "TEXT-WIDTH-BREAKPOINT-M1-01"
        exact_m1_baseline_run = 37921775934
        repo_sha = $actualCommit
        packet_sha256 = Sha $PacketFull
        worker_sha256 = Sha $Worker
        source_sha256 = [string]$fixture.expected_sha256
        seed_sha256 = [string]$seed.seed_sha256
        publisher_exe_sha256 = [string]$environment.publisher.sha256
        physical_arial_sha256 = $fontSha
        width_lower_pt = $low
        width_upper_pt = $high
        interval_width_pt = $high - $low
        line_index = 5
        line_end_low_utf16 = 139
        line_end_high_utf16 = 140
        controls_stable = $true
        save_reopen_stable = $true
        actual_arm_count = $arms.Count
        trace = $trace
        arm_summaries = @($arms | ForEach-Object {
            [ordered]@{
                id = [string]$_.arm_id
                width_pt = [double]$_.requested_width_pt
                line_count = [int]$_.fresh_reopen.line_count
                fifth_line_end_utf16 = [int]$_.fresh_reopen.lines[4].end
                break_signature = [string](Signature $_.fresh_reopen)
                output_sha256 = [string]$_.output_sha256
            }
        })
        status = "bounded_fifth_line_breakpoint_only"
        source_bytes_uploaded = $false
        carrier_authority_granted = $false
        product_visual_acceptance_granted = $false
    }
    Write-PubJson -Value $summary -Path (Join-Path $AnalysisDir "text-width-breakpoint-m2-01.json")
    @("experiment=$ExpectedId","arms=10","line=5","status=bounded_fifth_line_breakpoint_only","source_bytes_uploaded=false") |
        Set-Content -LiteralPath (Join-Path $LogDir "text-width-breakpoint-m2-01.txt") -Encoding ASCII

    $CurrentPhase = "finalize"
    Write-Stage "complete" "complete" "none"
    & (Join-Path $RepoRoot "tools/research-runner/finalize_native_run.ps1") -PacketPath $PacketFull -OutputRoot $OutputRoot
} catch {
    Write-Stage "invalid" $CurrentPhase $CurrentArm
    throw
}
