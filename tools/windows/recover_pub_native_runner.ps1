param(
    [string]$PacketPath = "tools/research-runner/experiments/pub-tlb-shape-effects-batch01.packet.json",
    [string]$ExpectedEnvironment = "publisher-2019",
    [string]$OutputRoot = "",
    [switch]$NoStartService
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

function Resolve-ServiceExecutable {
    param([Parameter(Mandatory = $true)]$Service)
    $raw = [string]$Service.PathName
    if ([string]::IsNullOrWhiteSpace($raw)) { return $null }
    if ($raw.StartsWith('"')) {
        $closing = $raw.IndexOf('"', 1)
        if ($closing -gt 1) { return $raw.Substring(1, $closing - 1) }
    }
    return ($raw -split "\s+", 2)[0]
}

function Get-RunnerCandidate {
    param([Parameter(Mandatory = $true)]$Service)
    $exe = Resolve-ServiceExecutable -Service $Service
    $root = $null
    $runnerFile = $null
    $runner = $null
    if ($exe) {
        $bin = Split-Path -Parent $exe
        if ($bin) {
            $root = Split-Path -Parent $bin
            $candidate = Join-Path $root ".runner"
            if (Test-Path -LiteralPath $candidate) {
                $runnerFile = $candidate
                try { $runner = Get-Content -LiteralPath $candidate -Raw | ConvertFrom-Json } catch { $runner = $null }
            }
        }
    }
    $githubUrl = $null
    $agentName = $null
    if ($runner) {
        if ($runner.PSObject.Properties.Name -contains "gitHubUrl") { $githubUrl = [string]$runner.gitHubUrl }
        if ($runner.PSObject.Properties.Name -contains "agentName") { $agentName = [string]$runner.agentName }
    }
    [pscustomobject]@{
        service = $Service
        executable = $exe
        root = $root
        runner_file = $runnerFile
        github_url = $githubUrl
        agent_name = $agentName
    }
}

function Write-Receipt {
    param([Parameter(Mandatory = $true)]$Value, [Parameter(Mandatory = $true)][string]$Path)
    $parent = Split-Path -Parent $Path
    if ($parent) { New-Item -ItemType Directory -Force -Path $parent | Out-Null }
    $Value | ConvertTo-Json -Depth 12 | Set-Content -LiteralPath $Path -Encoding utf8
}

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "../..")).Path
$packetAbs = if ([System.IO.Path]::IsPathRooted($PacketPath)) { $PacketPath } else { Join-Path $repoRoot $PacketPath }
if (-not (Test-Path -LiteralPath $packetAbs)) { throw "Packet not found: $packetAbs" }
if ([string]::IsNullOrWhiteSpace($OutputRoot)) { $OutputRoot = Join-Path $env:TEMP ("pub-native-runner-recovery-" + [DateTime]::UtcNow.ToString("yyyyMMdd-HHmmss")) }
New-Item -ItemType Directory -Force -Path $OutputRoot | Out-Null
$receiptPath = Join-Path $OutputRoot "runner-recovery.json"
$prepareRoot = Join-Path $OutputRoot "prepare"

$receipt = [ordered]@{
    schema = "chaptera.pub-native-runner-recovery.v1"
    captured_at_utc = [DateTime]::UtcNow.ToString("o")
    repository_root = $repoRoot
    expected_environment = $ExpectedEnvironment
    packet = [ordered]@{ path = $packetAbs; sha256 = (Get-FileHash -LiteralPath $packetAbs -Algorithm SHA256).Hash.ToLowerInvariant() }
    runner = [ordered]@{ service_count = 0; selected_service = $null; service_status_before = $null; service_status_after = $null; executable = $null; root = $null; runner_file = $null; agent_name = $null; github_url = $null; listener_present = $false }
    packet_validation = [ordered]@{ status = "not_run"; exit_code = $null }
    environment_validation = [ordered]@{ status = "not_run"; output_root = $prepareRoot; error = $null }
    verdict = "unknown"
}

try {
    $services = @(Get-CimInstance Win32_Service | Where-Object { [string]$_.Name -like "actions.runner.*" })
    $receipt.runner.service_count = $services.Count
    if ($services.Count -eq 0) {
        $receipt.verdict = "runner-service-not-installed-or-not-registered"
        Write-Receipt -Value $receipt -Path $receiptPath
        Write-Host "No actions.runner.* Windows service was found."
        Write-Host "This helper will not create a runner registration token or register a new runner."
        exit 3
    }

    $candidates = @($services | ForEach-Object { Get-RunnerCandidate -Service $_ })
    $repoCandidates = @($candidates | Where-Object { $_.github_url -and $_.github_url.TrimEnd("/") -ieq "https://github.com/Lalalalendia/rar2" })
    if ($repoCandidates.Count -eq 1) { $selected = $repoCandidates[0] }
    elseif ($candidates.Count -eq 1) { $selected = $candidates[0] }
    else {
        $summary = $candidates | ForEach-Object { "$($_.service.Name) agent=$($_.agent_name) github=$($_.github_url) root=$($_.root)" }
        $receipt.verdict = "ambiguous-runner-service"
        $receipt.runner.candidates = @($summary)
        Write-Receipt -Value $receipt -Path $receiptPath
        throw "Multiple actions.runner services exist and no unique rar2 registration could be selected."
    }

    $serviceName = [string]$selected.service.Name
    $receipt.runner.selected_service = $serviceName
    $receipt.runner.service_status_before = [string]$selected.service.State
    $receipt.runner.executable = $selected.executable
    $receipt.runner.root = $selected.root
    $receipt.runner.runner_file = $selected.runner_file
    $receipt.runner.agent_name = $selected.agent_name
    $receipt.runner.github_url = $selected.github_url

    if ([string]$selected.service.State -ne "Running") {
        if ($NoStartService) {
            $receipt.verdict = "runner-service-stopped"
            Write-Receipt -Value $receipt -Path $receiptPath
            exit 4
        }
        Write-Host "Starting self-hosted runner service: $serviceName"
        Start-Service -Name $serviceName
        $deadline = [DateTime]::UtcNow.AddSeconds(20)
        do {
            Start-Sleep -Milliseconds 500
            $serviceNow = Get-CimInstance Win32_Service -Filter "Name='$serviceName'"
        } while ($serviceNow.State -ne "Running" -and [DateTime]::UtcNow -lt $deadline)
        if ($serviceNow.State -ne "Running") {
            $receipt.runner.service_status_after = [string]$serviceNow.State
            $receipt.verdict = "runner-service-start-failed"
            Write-Receipt -Value $receipt -Path $receiptPath
            throw "Runner service did not reach Running state within 20 seconds."
        }
    }

    $serviceNow = Get-CimInstance Win32_Service -Filter "Name='$serviceName'"
    $receipt.runner.service_status_after = [string]$serviceNow.State
    $listener = @(Get-Process -Name "Runner.Listener" -ErrorAction SilentlyContinue)
    $receipt.runner.listener_present = ($listener.Count -gt 0)

    Push-Location $repoRoot
    try {
        $validationText = & python tools/research-runner/validate_packet.py --packet $packetAbs --expected-environment $ExpectedEnvironment 2>&1
        $validationExit = $LASTEXITCODE
        $receipt.packet_validation.exit_code = $validationExit
        $receipt.packet_validation.status = if ($validationExit -eq 0) { "success" } else { "failure" }
        $receipt.packet_validation.output = @($validationText | ForEach-Object { [string]$_ })
        if ($validationExit -ne 0) {
            $receipt.verdict = "packet-validation-failed"
            Write-Receipt -Value $receipt -Path $receiptPath
            exit 5
        }

        New-Item -ItemType Directory -Force -Path $prepareRoot | Out-Null
        try {
            & pwsh -NoProfile -File tools/research-runner/prepare_native_run.ps1 -PacketPath $packetAbs -OutputRoot $prepareRoot
            if ($LASTEXITCODE -ne 0) { throw "prepare_native_run.ps1 exited with code $LASTEXITCODE" }
            $receipt.environment_validation.status = "success"
        }
        catch {
            $receipt.environment_validation.status = "failure"
            $receipt.environment_validation.error = $_.Exception.Message
            $receipt.verdict = "publisher-environment-validation-failed"
            Write-Receipt -Value $receipt -Path $receiptPath
            throw
        }
    }
    finally { Pop-Location }

    if (-not [bool]$receipt.runner.listener_present) {
        $listener = @(Get-Process -Name "Runner.Listener" -ErrorAction SilentlyContinue)
        $receipt.runner.listener_present = ($listener.Count -gt 0)
    }
    if ($receipt.runner.service_status_after -ne "Running") {
        $receipt.verdict = "runner-service-not-running"
        Write-Receipt -Value $receipt -Path $receiptPath
        exit 6
    }
    if (-not [bool]$receipt.runner.listener_present) {
        $receipt.verdict = "runner-listener-not-observed"
        Write-Receipt -Value $receipt -Path $receiptPath
        exit 7
    }

    $receipt.verdict = "runner-and-publisher-environment-ready"
    Write-Receipt -Value $receipt -Path $receiptPath
    Write-Host ""
    Write-Host "PUB native runner recovery verdict: $($receipt.verdict)"
    Write-Host "Runner service: $serviceName"
    Write-Host "Agent name: $($receipt.runner.agent_name)"
    Write-Host "Publisher environment: $ExpectedEnvironment"
    Write-Host "Receipt: $receiptPath"
}
catch {
    if ($receipt.verdict -eq "unknown") { $receipt.verdict = "recovery-error" }
    $receipt.error = $_.Exception.Message
    Write-Receipt -Value $receipt -Path $receiptPath
    throw
}
