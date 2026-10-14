param(
    [Parameter(Mandatory=$true)][string]$PacketPath,
    [Parameter(Mandatory=$true)][string]$OutputRoot
)
Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"
$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot "../../..")).Path
Import-Module (Join-Path $RepoRoot "tools/windows/pub-runtime/PubRuntime.psm1") -Force
$ExpectedPacket = "tools/research-runner/experiments/text-width-hyphenation-m3b-01.packet.json"
$ExpectedId = "TEXT-WIDTH-HYPHENATION-M3B-01"
$PacketFull = Join-Path $RepoRoot $ExpectedPacket
$Worker = Join-Path $PSScriptRoot "text_width_breakpoint_m1_worker_01.ps1"
$Recovery = Join-Path $PSScriptRoot "text_width_hyphenation_m3b_recovery_01.ps1"
$PrivateDir = Join-Path $OutputRoot "private/text-width-m3b"
$AnalysisDir = Join-Path $OutputRoot "analysis"
$LogDir = Join-Path $OutputRoot "logs"
$Marker = Join-Path $env:USERPROFILE ".chaptera-publisher-hyphenation-quarantine"
$StagePath = Join-Path $AnalysisDir "text-width-m3b-suite-stage.json"
$RestorationPath = Join-Path $AnalysisDir "text-width-m3b-restoration.json"
New-Item -ItemType Directory -Force -Path $PrivateDir,$AnalysisDir,$LogDir | Out-Null
$CurrentPhase = "preflight"
$CurrentArm = "none"
$InterventionStarted = $false
$MarkerCleared = $false
$RestorationMode = "not_required"
function Sha([string]$Path) {
    return (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
}
function Write-Stage([string]$State,[string]$Phase,[string]$Arm) {
    Write-PubJson -Path $StagePath -Value ([ordered]@{
        schema = "chaptera.text-width-m3b-suite-stage.v1"
        experiment_id = $ExpectedId
        state = $State
        phase = $Phase
        arm_id = $Arm
        quarantine_cleared = $script:MarkerCleared
    })
}
function Signature($Snapshot) {
    return (@($Snapshot.lines | ForEach-Object {
        "$([int]$_.start):$([int]$_.end)"
    }) -join "|")
}
function Assert-FixedFont($Snapshot) {
    if (-not [bool]$Snapshot.visible_font_name_matches -or
        [math]::Abs([double]$Snapshot.visible_font_size_pt - 12.0) -gt 0.001 -or
        -not [bool]$Snapshot.visible_text_matches_expected -or
        [int]$Snapshot.auto_fit_mode -ne 0 -or
        [math]::Abs([double]$Snapshot.width_pt - 162.53125) -gt 0.001) {
        throw "m3b_font_text_width_or_autofit_drift"
    }
}
function Run-Child([ValidateSet("on","off","verify","restore")][string]$Case,
                   [string]$Source,[string]$SourceSha,[string]$ChildRoot) {
    $ps = Join-Path $env:SystemRoot "System32/WindowsPowerShell/v1.0/powershell.exe"
    if (-not (Test-Path -LiteralPath $ps -PathType Leaf)) { throw "m3b_powershell_unavailable" }
    New-Item -ItemType Directory -Force -Path $ChildRoot | Out-Null
    $quote = { param([string]$v) '"' + ($v -replace '"','') + '"' }
    $log = Join-Path $ChildRoot ("child-" + $Case)
    if ($Case -in @("on","off")) {
        $arguments = @("-NoProfile","-ExecutionPolicy","Bypass","-File",(& $quote $Worker),
            "-Mode","seed","-SourcePath",(& $quote $Source),"-OutputRoot",(& $quote $ChildRoot),
            "-ArmId","none","-ExpectedSourceSha",$SourceSha,
            "-CaptureHyphenation","-M3bSeedWidthPt","162.53125")
        if ($Case -eq "off") { $arguments += "-M3bOffDuringCreation" }
    } else {
        $arguments = @("-NoProfile","-ExecutionPolicy","Bypass","-File",(& $quote $Recovery),
            "-Mode",$Case,"-OutputRoot",(& $quote $ChildRoot))
    }
    $started = [DateTime]::UtcNow
    $child = Start-Process -FilePath $ps -ArgumentList $arguments -PassThru -RedirectStandardOutput ($log + ".stdout.txt") -RedirectStandardError ($log + ".stderr.txt")
    if (-not $child.WaitForExit(180000)) {
        if (-not $child.HasExited) {
            try { Stop-Process -Id $child.Id -Force -ErrorAction Stop }
            catch { throw "m3b_child_ownership_cleanup_failed" }
            [void]$child.WaitForExit(10000)
        }
        $owned = @(Get-Process -Name MSPUB -ErrorAction SilentlyContinue | Where-Object {
            try { $_.StartTime.ToUniversalTime() -ge $started.AddSeconds(-2) }
            catch { $false }
        })
        if ($owned.Count -eq 1) {
            try { Stop-Process -Id $owned[0].Id -Force -ErrorAction Stop }
            catch { throw "m3b_publisher_cleanup_failed" }
        } else { throw "m3b_publisher_ownership_uncertain" }
        throw "m3b_child_timeout"
    }
    $child.WaitForExit()
    $child.Refresh()
    if ([int]$child.ExitCode -ne 0) { throw "m3b_child_invalid_$Case" }
    Start-Sleep -Seconds 2
    if (@(Get-Process -Name MSPUB -ErrorAction SilentlyContinue).Count -ne 0) {
        throw "m3b_publisher_still_running_$Case"
    }
}
function Read-Seed([string]$Root) {
    $path = Join-Path $Root "analysis/text-width-m1-seed.json"
    $data = Get-Content -LiteralPath $path -Raw | ConvertFrom-Json
    if ($data.schema -cne "chaptera.text-width-m1-seed.v1" -or
        $null -eq $data.snapshot_after_save) {
        throw "m3b_seed_receipt_missing_or_schema"
    }
    return $data
}
function Restore-And-Verify {
    if (@(Get-Process -Name MSPUB -ErrorAction SilentlyContinue).Count -ne 0) {
        throw "m3b_uncertain_publisher_ownership_quarantine"
    }
    $recoveryRoot = Join-Path $PrivateDir "recovery"
    try {
        Run-Child "verify" "" "" $recoveryRoot
        $script:RestorationMode = "worker_restoration_independently_verified"
    } catch {
        if (@(Get-Process -Name MSPUB -ErrorAction SilentlyContinue).Count -ne 0) {
            throw "m3b_recovery_process_ownership_uncertain"
        }
        Run-Child "restore" "" "" $recoveryRoot
        Run-Child "verify" "" "" $recoveryRoot
        $script:RestorationMode = "recovery_process_restored_and_verified"
    }
    $receipt = Get-Content -LiteralPath (Join-Path $recoveryRoot "analysis/text-width-m3b-verify.json") -Raw | ConvertFrom-Json
    if (-not [bool]$receipt.verified -or
        -not [bool]$receipt.settings_after.auto_hyphenate -or
        [math]::Abs([double]$receipt.settings_after.hyphenation_zone_pt - 18.0) -gt 0.000001) {
        throw "m3b_independent_restore_receipt_invalid"
    }
    Remove-Item -LiteralPath $Marker -Force -ErrorAction Stop
    if (Test-Path -LiteralPath $Marker) { throw "m3b_marker_removal_unverified" }
    $script:MarkerCleared = $true
    Write-PubJson -Path $RestorationPath -Value ([ordered]@{
        schema = "chaptera.text-width-m3b-restoration.v1"
        experiment_id = $ExpectedId
        restored_auto_hyphenate = $true
        restored_hyphenation_zone_pt = 18.0
        independent_fresh_process_verified = $true
        recovery_strategy = $script:RestorationMode
        quarantine_cleared = $true
        source_bytes_uploaded = $false
    })
}
try {
    Write-Stage "running" $CurrentPhase $CurrentArm
    if (Test-Path -LiteralPath $Marker -PathType Leaf) { throw "m3b_preexisting_quarantine" }
    if ((Resolve-Path -LiteralPath $PacketPath).Path -ne (Resolve-Path -LiteralPath $PacketFull).Path) {
        throw "m3b_packet_not_allowlisted"
    }
    $packet = Get-Content -LiteralPath $PacketFull -Raw | ConvertFrom-Json
    if ($packet.id -cne $ExpectedId -or
        $packet.publisher_environment -cne "publisher-2019" -or
        [string]$packet.operation.script -cne "tools/research-runner/operations/text_width_hyphenation_m3b_01.ps1") {
        throw "m3b_packet_identity_invalid"
    }
    if ($env:GITHUB_REF -cne "refs/heads/main" -or
        [string]::IsNullOrWhiteSpace($env:GITHUB_SHA)) {
        throw "m3b_requires_trusted_main"
    }
    $actualCommit = (& git -C $RepoRoot rev-parse HEAD).Trim()
    if ($LASTEXITCODE -ne 0 -or $actualCommit -cne $env:GITHUB_SHA) {
        throw "m3b_checkout_not_trusted"
    }
    & python (Join-Path $RepoRoot "tools/research-runner/validate_packet.py") --packet $ExpectedPacket --expected-environment publisher-2019 | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "m3b_packet_contract_failed" }
    if (@(Get-Process -Name MSPUB -ErrorAction SilentlyContinue).Count -ne 0) {
        throw "m3b_publisher_busy_before_run"
    }
    $CurrentPhase = "prepare"
    Write-Stage "running" $CurrentPhase $CurrentArm
    & (Join-Path $RepoRoot "tools/research-runner/prepare_native_run.ps1") -PacketPath $PacketFull -OutputRoot $OutputRoot
    $environment = Get-Content -LiteralPath (Join-Path $OutputRoot "environment.json") -Raw | ConvertFrom-Json
    $registry = Get-Content -LiteralPath (Join-Path $RepoRoot "tools/pub-re/native-fixtures.json") -Raw | ConvertFrom-Json
    if ($registry.schema -cne "chaptera.pub-re-native-fixture-registry.v1") {
        throw "m3b_fixture_registry_invalid"
    }
    $matches = @($registry.fixtures | Where-Object { $_.alias -eq "sample-newsletter" })
    if ($matches.Count -ne 1) { throw "m3b_fixture_alias_ambiguous" }
    $fixture = $matches[0]
    if ([string]$fixture.expected_sha256 -cne "6a825ba26ba35d6e885acdc62e859591ed37cb0ff7480b554b9cb362b644dfcf" -or
        [int64]$fixture.expected_byte_len -ne 291840) {
        throw "m3b_fixture_not_allowlisted"
    }
    $uri = [Uri]([string]$fixture.download_url)
    if ($uri.Scheme -cne "https" -or $uri.Host -cne "raw.githubusercontent.com") {
        throw "m3b_fixture_remote_untrusted"
    }
    $source = Join-Path $PrivateDir "source.pub"
    Invoke-WebRequest -Uri $uri.AbsoluteUri -OutFile $source -UseBasicParsing
    if ((Sha $source) -cne [string]$fixture.expected_sha256 -or
        (Get-Item -LiteralPath $source).Length -ne 291840) { throw "m3b_source_not_pinned" }
    $font = Join-Path $env:WINDIR "Fonts/arial.ttf"
    if (-not (Test-Path -LiteralPath $font -PathType Leaf) -or
        (Sha $font) -cne "c9b76220a5be42ead4733611e417cd65c5fd8aeaa33eb56576ac378a37d130a1") {
        throw "m3b_physical_font_drift"
    }

    $CurrentPhase = "on_control"
    $CurrentArm = "on"
    Write-Stage "running" $CurrentPhase $CurrentArm
    $onRoot = Join-Path $PrivateDir "on-run"
    Run-Child "on" $source ([string]$fixture.expected_sha256) $onRoot
    $on = Read-Seed $onRoot
    foreach ($property in @("snapshot_before_save","snapshot_after_save","snapshot_fresh_reopen")) {
        $obs = $on.$property
        Assert-FixedFont $obs
        if (-not [bool]$obs.hyphenation_options.auto_hyphenate -or
            [math]::Abs([double]$obs.hyphenation_options.hyphenation_zone_pt - 18.0) -gt 0.000001) {
            throw "m3b_on_control_options_changed"
        }
    }
    $onSignature = Signature $on.snapshot_after_save
    if ($onSignature -cne "0:28|28:57|57:86|86:112|112:140|140:155" -or
        (Signature $on.snapshot_fresh_reopen) -cne $onSignature) {
        throw "m3b_on_direct_creation_not_m2_upper_edge"
    }
    Copy-Item -LiteralPath (Join-Path $onRoot "analysis/text-width-m1-seed.json") -Destination (Join-Path $AnalysisDir "text-width-m3b-on.json") -Force

    $CurrentPhase = "off_intervention"
    $CurrentArm = "off"
    Write-Stage "running" $CurrentPhase $CurrentArm
    "TEXT-WIDTH-HYPHENATION-M3B-01" | Set-Content -LiteralPath $Marker -Encoding ASCII
    $InterventionStarted = $true
    $offRoot = Join-Path $PrivateDir "off-run"
    try {
        Run-Child "off" $source ([string]$fixture.expected_sha256) $offRoot
    } finally {
        $CurrentPhase = "restoration"
        Write-Stage "running" $CurrentPhase $CurrentArm
        Restore-And-Verify
    }
    $off = Read-Seed $offRoot
    foreach ($property in @("snapshot_before_save","snapshot_after_save","snapshot_fresh_reopen")) {
        Assert-FixedFont $off.$property
    }
    if ([bool]$off.snapshot_before_save.hyphenation_options.auto_hyphenate -or
        [bool]$off.snapshot_after_save.hyphenation_options.auto_hyphenate -or
        -not [bool]$off.snapshot_fresh_reopen.hyphenation_options.auto_hyphenate) {
        throw "m3b_off_or_restored_option_not_confirmed"
    }
    if ([math]::Abs([double]$off.snapshot_after_save.hyphenation_options.hyphenation_zone_pt - 18.0) -gt 0.000001 -or
        [string]$off.snapshot_after_save.text_utf16le_sha256 -cne [string]$on.snapshot_after_save.text_utf16le_sha256 -or
        (Sha $source) -cne [string]$fixture.expected_sha256) {
        throw "m3b_text_zone_or_source_confounded"
    }
    Copy-Item -LiteralPath (Join-Path $offRoot "analysis/text-width-m1-seed.json") -Destination (Join-Path $AnalysisDir "text-width-m3b-off.json") -Force
    $CurrentPhase = "comparison"
    $CurrentArm = "none"
    Write-Stage "running" $CurrentPhase $CurrentArm
    $offSignature = Signature $off.snapshot_after_save
    $offFreshSignature = Signature $off.snapshot_fresh_reopen
    $effect = ($onSignature -cne $offSignature)
    $verdict = if ($effect) {
        "app_hyphenation_changes_new_story_layout"
    } else { "no_detectable_app_option_effect_on_new_story" }
    Write-PubJson -Path (Join-Path $AnalysisDir "text-width-hyphenation-m3b-01.json") -Value ([ordered]@{
        schema = "chaptera.text-width-hyphenation-m3b.v1"
        experiment_id = $ExpectedId
        repo_sha = $actualCommit
        packet_sha256 = Sha $PacketFull
        source_sha256 = [string]$fixture.expected_sha256
        publisher_exe_sha256 = [string]$environment.publisher.sha256
        physical_arial_sha256 = Sha $font
        requested_width_pt = 162.53125
        hyphenation_zone_pt = 18.0
        on_auto_hyphenate = $true
        off_auto_hyphenate = $false
        on_save_break_signature = $onSignature
        off_save_break_signature = $offSignature
        off_fresh_reopen_break_signature = $offFreshSignature
        application_setting_changed_before_new_story = $true
        story_policy_observed = $false
        causal_effect_at_app_option_scope = $effect
        off_layout_stable_after_reopen_under_on = ($offSignature -ceq $offFreshSignature)
        independent_restoration_verified = $MarkerCleared
        status = $verdict
        source_bytes_uploaded = $false
        carrier_authority_granted = $false
        product_visual_acceptance_granted = $false
    })
    @("experiment=$ExpectedId","arms=2","status=$verdict",
      "independent_restoration_verified=$MarkerCleared","source_bytes_uploaded=false") |
        Set-Content -LiteralPath (Join-Path $LogDir "text-width-hyphenation-m3b-01.txt") -Encoding ASCII
    $CurrentPhase = "finalize"
    Write-Stage "complete" "complete" "none"
    & (Join-Path $RepoRoot "tools/research-runner/finalize_native_run.ps1") -PacketPath $PacketFull -OutputRoot $OutputRoot
} catch {
    if ($InterventionStarted -and -not $MarkerCleared) {
        Write-PubJson -Path $RestorationPath -Value ([ordered]@{
            schema = "chaptera.text-width-m3b-restoration.v1"
            experiment_id = $ExpectedId
            independent_fresh_process_verified = $false
            quarantine_cleared = $false
            source_bytes_uploaded = $false
        })
    }
    Write-Stage "invalid" $CurrentPhase $CurrentArm
    throw
}
