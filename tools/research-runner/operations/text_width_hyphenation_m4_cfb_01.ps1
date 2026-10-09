param(
    [Parameter(Mandatory=$true)][string]$PacketPath,
    [Parameter(Mandatory=$true)][string]$OutputRoot
)
Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot "../../..")).Path
Import-Module (Join-Path $RepoRoot "tools/windows/pub-runtime/PubRuntime.psm1") -Force
$ExpectedId = "TEXT-WIDTH-HYPHENATION-M4-CFB-01"
$ExpectedPacket = "tools/research-runner/experiments/text-width-hyphenation-m4-cfb-01.packet.json"
$PacketFull = Join-Path $RepoRoot $ExpectedPacket
$M3bPacket = Join-Path $RepoRoot "tools/research-runner/experiments/text-width-hyphenation-m3b-01.packet.json"
$M3bOperation = Join-Path $PSScriptRoot "text_width_hyphenation_m3b_01.ps1"
$Worker = Join-Path $PSScriptRoot "text_width_breakpoint_m1_worker_01.ps1"
$PrivateDir = Join-Path $OutputRoot "private/text-width-m4"
$AnalysisDir = Join-Path $OutputRoot "analysis"
$LogDir = Join-Path $OutputRoot "logs"
$StagePath = Join-Path $AnalysisDir "text-width-m4-suite-stage.json"
$Marker = Join-Path $env:USERPROFILE ".chaptera-publisher-hyphenation-quarantine"
$CurrentPhase = "preflight"
New-Item -ItemType Directory -Force -Path $PrivateDir,$AnalysisDir,$LogDir | Out-Null

function Sha([string]$Path) {
    return (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
}
function Stage([string]$State,[string]$Phase) {
    Write-PubJson -Path $StagePath -Value ([ordered]@{
        schema = "chaptera.text-width-m4-suite-stage.v1"
        experiment_id = $ExpectedId
        state = $State
        phase = $Phase
        source_bytes_uploaded = $false
    })
}
function Signature($Snapshot) {
    return (@($Snapshot.lines | ForEach-Object {
        "$([int]$_.start):$([int]$_.end)"
    }) -join "|")
}
function Assert-Frame($Obs,[bool]$ExpectedHyphenation) {
    if ($null -eq $Obs -or -not [bool]$Obs.visible_font_name_matches -or
        -not [bool]$Obs.visible_text_matches_expected -or
        [math]::Abs([double]$Obs.visible_font_size_pt - 12.0) -gt 0.001 -or
        [int]$Obs.auto_fit_mode -ne 0 -or
        [math]::Abs([double]$Obs.width_pt - 162.53125) -gt 0.001 -or
        [int]$Obs.line_count -ne 6 -or
        $null -eq $Obs.hyphenation_options -or
        [bool]$Obs.hyphenation_options.auto_hyphenate -ne $ExpectedHyphenation -or
        [math]::Abs([double]$Obs.hyphenation_options.hyphenation_zone_pt - 18.0) -gt 0.000001) {
        throw "m4_frame_font_width_story_or_setting_drift"
    }
}
function Run-On2([string]$Source,[string]$SourceSha,[string]$ChildRoot) {
    $ps = Join-Path $env:SystemRoot "System32/WindowsPowerShell/v1.0/powershell.exe"
    if (-not (Test-Path -LiteralPath $ps -PathType Leaf)) { throw "m4_windows_powershell_unavailable" }
    New-Item -ItemType Directory -Force -Path $ChildRoot | Out-Null
    $quote = { param([string]$v) '"' + ($v -replace '"','') + '"' }
    $arguments = @("-NoProfile","-ExecutionPolicy","Bypass","-File",(& $quote $Worker),
        "-Mode","seed","-SourcePath",(& $quote $Source),"-OutputRoot",(& $quote $ChildRoot),
        "-ArmId","none","-ExpectedSourceSha",$SourceSha,
        "-CaptureHyphenation","-M3bSeedWidthPt","162.53125")
    $logBase = Join-Path $ChildRoot "on2-child"
    $start = [DateTime]::UtcNow
    $child = Start-Process -FilePath $ps -ArgumentList $arguments -PassThru -RedirectStandardOutput ($logBase + ".stdout.txt") -RedirectStandardError ($logBase + ".stderr.txt")
    if (-not $child.WaitForExit(180000)) {
        if (-not $child.HasExited) {
            try { Stop-Process -Id $child.Id -Force -ErrorAction Stop }
            catch { throw "m4_on2_child_cleanup_failed" }
            [void]$child.WaitForExit(10000)
        }
        $owned = @(Get-Process -Name MSPUB -ErrorAction SilentlyContinue | Where-Object {
            try { $_.StartTime.ToUniversalTime() -ge $start.AddSeconds(-2) }
            catch { $false }
        })
        if ($owned.Count -eq 1) {
            try { Stop-Process -Id $owned[0].Id -Force -ErrorAction Stop }
            catch { throw "m4_on2_publisher_cleanup_failed" }
        } else {
            throw "m4_on2_publisher_ownership_uncertain"
        }
        throw "m4_on2_child_timeout"
    }
    $child.WaitForExit()
    $child.Refresh()
    if ([int]$child.ExitCode -ne 0) { throw "m4_on2_invalid" }
    Start-Sleep -Seconds 2
    if (@(Get-Process -Name MSPUB -ErrorAction SilentlyContinue).Count -ne 0) {
        throw "m4_on2_publisher_still_running"
    }
}
function Read-Seed([string]$Path) {
    $doc = Get-Content -LiteralPath $Path -Raw | ConvertFrom-Json
    if ($doc.schema -cne "chaptera.text-width-m1-seed.v1" -or
        $null -eq $doc.snapshot_after_save -or
        $null -eq $doc.snapshot_fresh_reopen) {
        throw "m4_invalid_seed_receipt"
    }
    return $doc
}
function Write-PrivateComparison([string]$Label,[string]$Before,[string]$After,[string]$DiffRoot) {
    $manifestPath = Join-Path $DiffRoot ("manifest-" + $Label + ".json")
    $rawPath = Join-Path $DiffRoot ("raw-diff-" + $Label + ".json")
    $safePath = Join-Path $AnalysisDir ("text-width-m4-cfb-" + $Label + ".json")
    # The manifest contains private absolute PUB paths and is never copied to
    # the public artifact allowlist. PUB RE v0 emits hashes/ranges, not bytes.
    Write-PubJson -Path $manifestPath -Value ([ordered]@{
        schema = "chaptera.pub-re-experiment.v1"
        experiment_id = "M4-" + $Label
        question = "Which logical CFB streams differ between controlled hyphenation Stories?"
        before = [ordered]@{path=$Before;expected_sha256=(Sha $Before)}
        after = [ordered]@{path=$After;expected_sha256=(Sha $After)}
        policy = [ordered]@{max_stream_bytes=67108864;max_changed_ranges_per_stream=24}
    })
    $binary = Join-Path $env:RUNNER_TEMP "chaptera-pub-re-m4-target/debug/pub-re.exe"
    if (-not (Test-Path -LiteralPath $binary -PathType Leaf)) {
        throw "m4_pub_re_binary_not_built"
    }
    & $binary analyze --manifest $manifestPath --output $rawPath 2>&1 | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "m4_pub_re_analyzer_failed" }
    $raw = Get-Content -LiteralPath $rawPath -Raw | ConvertFrom-Json
    if ($raw.schema -cne "chaptera.pub-re-receipt.v1" -or
        -not [bool]$raw.invariants.expected_input_hashes_verified -or
        -not [bool]$raw.invariants.deterministic_ordering -or
        [bool]$raw.invariants.raw_document_bytes_emitted -or
        [bool]$raw.invariants.raw_stream_bytes_emitted -or
        [bool]$raw.invariants.absolute_input_paths_emitted) {
        throw "m4_pub_re_receipt_not_source_safe"
    }
    $changes = @($raw.cfb.changed_streams | ForEach-Object {
        [ordered]@{
            path = [string]$_.path
            before_len = [long]$_.before_len
            after_len = [long]$_.after_len
            before_sha256 = [string]$_.before_sha256
            after_sha256 = [string]$_.after_sha256
            equal_length_changed_byte_count = $_.equal_length_changed_byte_count
            common_prefix_len = [long]$_.common_prefix_len
            common_suffix_len = [long]$_.common_suffix_len
            changed_ranges = @($_.changed_ranges | Select-Object -First 12)
            changed_ranges_truncated = [bool]$_.changed_ranges_truncated -or
                @($_.changed_ranges).Count -gt 12
        }
    })
    Write-PubJson -Path $safePath -Value ([ordered]@{
        schema = "chaptera.text-width-m4-cfb-pair.v1"
        experiment_id = $ExpectedId
        comparison = $Label
        analyzer_schema = [string]$raw.schema
        analyzer_version = [string]$raw.analyzer_version
        cfb_status = [string]$raw.status
        before_sha256 = [string]$raw.inputs.before.sha256
        after_sha256 = [string]$raw.inputs.after.sha256
        changed_stream_count = @($raw.cfb.changed_streams).Count
        changed_streams = $changes
        added_entries = @($raw.cfb.added_entries | Select-Object -First 20)
        removed_entries = @($raw.cfb.removed_entries | Select-Object -First 20)
        entry_list_truncated = (@($raw.cfb.added_entries).Count -gt 20 -or
            @($raw.cfb.removed_entries).Count -gt 20)
        changed_entry_shape_count = @($raw.cfb.changed_entry_shapes).Count
        source_bytes_uploaded = $false
        raw_pub_uploaded = $false
    })
    if ((Get-Item -LiteralPath $safePath).Length -gt 131072) {
        throw "m4_source_safe_cfb_receipt_oversize"
    }
    return $raw
}

try {
    Stage "running" $CurrentPhase
    if ((Resolve-Path -LiteralPath $PacketPath).Path -ne
        (Resolve-Path -LiteralPath $PacketFull).Path) {
        throw "m4_packet_not_allowlisted"
    }
    $packet = Get-Content -LiteralPath $PacketFull -Raw | ConvertFrom-Json
    if ($packet.id -cne $ExpectedId -or
        $packet.publisher_environment -cne "publisher-2019" -or
        [string]$packet.operation.script -cne "tools/research-runner/operations/text_width_hyphenation_m4_cfb_01.ps1") {
        throw "m4_packet_identity_invalid"
    }
    if ($env:GITHUB_REF -cne "refs/heads/main" -or
        [string]::IsNullOrWhiteSpace($env:GITHUB_SHA)) {
        throw "m4_requires_trusted_main"
    }
    $actualCommit = (& git -C $RepoRoot rev-parse HEAD).Trim()
    if ($LASTEXITCODE -ne 0 -or $actualCommit -cne $env:GITHUB_SHA) {
        throw "m4_checkout_not_trusted"
    }
    if (Test-Path -LiteralPath $Marker -PathType Leaf) {
        throw "m4_host_quarantine_present"
    }
    if (@(Get-Process -Name MSPUB -ErrorAction SilentlyContinue).Count -ne 0) {
        throw "m4_publisher_busy_before_run"
    }
    & python (Join-Path $RepoRoot "tools/research-runner/validate_packet.py") --packet $ExpectedPacket --expected-environment publisher-2019 | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "m4_packet_validation_failed" }

    $CurrentPhase = "prepare_and_build"
    Stage "running" $CurrentPhase
    & (Join-Path $RepoRoot "tools/research-runner/prepare_native_run.ps1") -PacketPath $PacketFull -OutputRoot $OutputRoot
    $environment = Get-Content -LiteralPath (Join-Path $OutputRoot "environment.json") -Raw | ConvertFrom-Json
    if ([string]$environment.publisher.sha256 -cne "e1ef8811b85b82045f37c4173b92726101be3a25e550b0dcb9f178df834ab20b") {
        throw "m4_publisher_binary_drift"
    }
    $font = Join-Path $env:WINDIR "Fonts/arial.ttf"
    if (-not (Test-Path -LiteralPath $font -PathType Leaf) -or
        (Sha $font) -cne "c9b76220a5be42ead4733611e417cd65c5fd8aeaa33eb56576ac378a37d130a1") {
        throw "m4_physical_arial_drift"
    }
    $cargo = Get-Command cargo -ErrorAction Stop
    if ($null -eq $cargo) { throw "m4_cargo_unavailable" }
    $targetDir = Join-Path $env:RUNNER_TEMP "chaptera-pub-re-m4-target"
    & cargo build --quiet --manifest-path (Join-Path $RepoRoot "tools/pub-re/Cargo.toml") --target-dir $targetDir --bin pub-re 2>&1 | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "m4_pub_re_build_failed" }
    if (-not (Test-Path -LiteralPath (Join-Path $targetDir "debug/pub-re.exe") -PathType Leaf)) {
        throw "m4_pub_re_binary_missing"
    }

    # Reuse the fully verified M3b On1/Off transaction and independent rollback,
    # rather than introducing a second global-option writer or restore path.
    $CurrentPhase = "m3b_on_off_and_restore"
    Stage "running" $CurrentPhase
    $M3bRoot = Join-Path $PrivateDir "m3b"
    & $M3bOperation -PacketPath $M3bPacket -OutputRoot $M3bRoot
    $m3bSummary = Get-Content -LiteralPath (Join-Path $M3bRoot "analysis/text-width-hyphenation-m3b-01.json") -Raw | ConvertFrom-Json
    $restoration = Get-Content -LiteralPath (Join-Path $M3bRoot "analysis/text-width-m3b-restoration.json") -Raw | ConvertFrom-Json
    if ($m3bSummary.status -cne "app_hyphenation_changes_new_story_layout" -or
        -not [bool]$m3bSummary.independent_restoration_verified -or
        -not [bool]$restoration.independent_fresh_process_verified -or
        -not [bool]$restoration.quarantine_cleared -or
        [math]::Abs([double]$restoration.restored_hyphenation_zone_pt - 18.0) -gt 0.000001 -or
        -not [bool]$restoration.restored_auto_hyphenate -or
        (Test-Path -LiteralPath $Marker -PathType Leaf)) {
        throw "m4_m3b_native_or_restore_not_verified"
    }
    Write-PubJson -Path (Join-Path $AnalysisDir "text-width-m4-restoration.json") -Value ([ordered]@{
        schema = "chaptera.text-width-m4-restoration.v1"
        experiment_id = $ExpectedId
        independent_fresh_process_verified = [bool]$restoration.independent_fresh_process_verified
        quarantine_cleared = [bool]$restoration.quarantine_cleared
        auto_hyphenate = [bool]$restoration.restored_auto_hyphenate
        hyphenation_zone_pt = [double]$restoration.restored_hyphenation_zone_pt
        strategy = [string]$restoration.recovery_strategy
        source_bytes_uploaded = $false
    })
    $M3bPrivate = Join-Path $M3bRoot "private/text-width-m3b"
    $source = Join-Path $M3bPrivate "source.pub"
    $on1pub = Join-Path $M3bPrivate "on-run/private/text-width-m1/seed.pub"
    $offpub = Join-Path $M3bPrivate "off-run/private/text-width-m1/seed.pub"
    $sourceSha = "6a825ba26ba35d6e885acdc62e859591ed37cb0ff7480b554b9cb362b644dfcf"
    if (-not (Test-Path -LiteralPath $on1pub -PathType Leaf) -or
        -not (Test-Path -LiteralPath $offpub -PathType Leaf) -or
        (Sha $source) -cne $sourceSha) {
        throw "m4_m3b_private_pub_or_source_missing"
    }
    $on1 = Read-Seed (Join-Path $M3bRoot "analysis/text-width-m3b-on.json")
    $off = Read-Seed (Join-Path $M3bRoot "analysis/text-width-m3b-off.json")
    if ((Sha $on1pub) -cne [string]$on1.seed_sha256 -or
        (Sha $offpub) -cne [string]$off.seed_sha256) {
        throw "m4_m3b_pub_receipt_hash_mismatch"
    }

    $CurrentPhase = "on2_new_story_control"
    Stage "running" $CurrentPhase
    $On2Root = Join-Path $PrivateDir "on2"
    Run-On2 $source $sourceSha $On2Root
    $on2 = Read-Seed (Join-Path $On2Root "analysis/text-width-m1-seed.json")
    $on2pub = Join-Path $On2Root "private/text-width-m1/seed.pub"
    if ((Sha $on2pub) -cne [string]$on2.seed_sha256) {
        throw "m4_on2_pub_receipt_hash_mismatch"
    }
    $onSig = "0:28|28:57|57:86|86:112|112:140|140:155"
    $offSig = "0:28|28:57|57:86|86:107|107:131|131:155"
    foreach ($row in @(
        [pscustomobject]@{receipt=$on1;option=$true;signature=$onSig},
        [pscustomobject]@{receipt=$off;option=$false;signature=$offSig},
        [pscustomobject]@{receipt=$on2;option=$true;signature=$onSig}
    )) {
        $item = $row.receipt
        foreach ($phase in @("snapshot_before_save","snapshot_after_save")) {
            $obs = $item.$phase
            Assert-Frame $obs $row.option
            if ((Signature $obs) -cne $row.signature) {
                throw "m4_before_after_save_reference_drift"
            }
        }
        Assert-Frame $item.snapshot_fresh_reopen $true
        if ((Signature $item.snapshot_fresh_reopen) -cne $row.signature) {
            throw "m4_reopen_line_signature_drift"
        }
        if ((Sha $source) -cne $sourceSha) { throw "m4_public_fixture_mutated" }
    }
    $shaText = [string]$on1.snapshot_after_save.text_utf16le_sha256
    foreach ($item in @($off,$on2)) {
        if ([string]$item.snapshot_after_save.text_utf16le_sha256 -cne $shaText -or
            [math]::Abs([double]$item.snapshot_after_save.width_pt - [double]$on1.snapshot_after_save.width_pt) -gt 0.00001) {
            throw "m4_text_or_stored_width_confounded"
        }
        foreach ($margin in @("margin_left_pt","margin_right_pt","margin_top_pt","margin_bottom_pt")) {
            if ([math]::Abs([double]$item.snapshot_after_save.$margin -
                [double]$on1.snapshot_after_save.$margin) -gt 0.00001) {
                throw "m4_inset_confounded"
            }
        }
    }
    Copy-Item -LiteralPath (Join-Path $M3bRoot "analysis/text-width-m3b-on.json") -Destination (Join-Path $AnalysisDir "text-width-m4-on1.json") -Force
    Copy-Item -LiteralPath (Join-Path $M3bRoot "analysis/text-width-m3b-off.json") -Destination (Join-Path $AnalysisDir "text-width-m4-off.json") -Force
    Copy-Item -LiteralPath (Join-Path $On2Root "analysis/text-width-m1-seed.json") -Destination (Join-Path $AnalysisDir "text-width-m4-on2.json") -Force

    $CurrentPhase = "source_safe_cfb_differential"
    Stage "running" $CurrentPhase
    $DiffRoot = Join-Path $PrivateDir "diff"
    New-Item -ItemType Directory -Force -Path $DiffRoot | Out-Null
    $control = Write-PrivateComparison "on1-on2" $on1pub $on2pub $DiffRoot
    $treat1 = Write-PrivateComparison "on1-off" $on1pub $offpub $DiffRoot
    $treat2 = Write-PrivateComparison "off-on2" $offpub $on2pub $DiffRoot
    $controlPaths = @($control.cfb.changed_streams | ForEach-Object { [string]$_.path } | Sort-Object -Unique)
    $treat1Paths = @($treat1.cfb.changed_streams | ForEach-Object { [string]$_.path } | Sort-Object -Unique)
    $treat2Paths = @($treat2.cfb.changed_streams | ForEach-Object { [string]$_.path } | Sort-Object -Unique)
    $sharedTreatmentPaths = @($treat1Paths | Where-Object { $treat2Paths -ccontains $_ })
    $tentativePaths = @($sharedTreatmentPaths | Where-Object { $controlPaths -cnotcontains $_ })
    $confoundedPaths = @($sharedTreatmentPaths | Where-Object { $controlPaths -ccontains $_ })
    $verdict = if ($tentativePaths.Count -gt 0) {
        "candidate_streams_not_changed_in_on_on_control"
    } else {
        "no_stream_unique_to_treatment_at_cfb_level"
    }
    Write-PubJson -Path (Join-Path $AnalysisDir "text-width-hyphenation-m4-cfb-01.json") -Value ([ordered]@{
        schema = "chaptera.text-width-hyphenation-m4-cfb.v1"
        experiment_id = $ExpectedId
        repo_sha = $actualCommit
        source_sha256 = $sourceSha
        physical_arial_sha256 = Sha $font
        publisher_exe_sha256 = [string]$environment.publisher.sha256
        input_pub_sha256 = [ordered]@{
            on1 = Sha $on1pub
            off = Sha $offpub
            on2 = Sha $on2pub
        }
        text_utf16le_sha256 = $shaText
        actual_width_pt = [double]$on1.snapshot_after_save.width_pt
        on1_breaks = $onSig
        off_breaks = $offSig
        on2_breaks = $onSig
        on_off_causal_effect_reproduced = $true
        restored_global_hyphenation_verified = $true
        control_changed_stream_count = $controlPaths.Count
        treatment_on1_off_changed_stream_count = $treat1Paths.Count
        treatment_off_on2_changed_stream_count = $treat2Paths.Count
        treatment_both_changed_paths = $sharedTreatmentPaths
        treatment_only_candidate_streams = $tentativePaths
        treatment_also_changed_in_control = $confoundedPaths
        stream_level_verdict = $verdict
        rule_not_established = "CFB stream-only changes do not establish record/property or Story carrier authority"
        raw_pub_uploaded = $false
        source_bytes_uploaded = $false
        product_visual_acceptance_granted = $false
    })
    @("experiment=$ExpectedId","arms=3","status=$verdict",
      "source_bytes_uploaded=false","global_option_restoration=independently_verified") |
        Set-Content -LiteralPath (Join-Path $LogDir "text-width-hyphenation-m4-cfb-01.txt") -Encoding ASCII

    $CurrentPhase = "finalize"
    Stage "complete" $CurrentPhase
    & (Join-Path $RepoRoot "tools/research-runner/finalize_native_run.ps1") -PacketPath $PacketFull -OutputRoot $OutputRoot
} catch {
    # All detailed failures stay in private child output. Public stage contains
    # only a bounded phase. Never claim a source law from failed evidence.
    Stage "invalid" $CurrentPhase
    throw "m4_invalid_$CurrentPhase"
}
