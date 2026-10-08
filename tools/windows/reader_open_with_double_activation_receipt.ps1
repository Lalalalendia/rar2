[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [ValidateScript({ Test-Path -LiteralPath $_ -PathType Leaf })]
    [string]$SourceA,

    [Parameter(Mandatory = $true)]
    [ValidateScript({ Test-Path -LiteralPath $_ -PathType Leaf })]
    [string]$SourceB,

    [string]$ReaderExe = (Join-Path $env:LOCALAPPDATA "Programs\Chaptera PUB Reader\current\chaptera-reader.exe"),

    [string]$Output = (Join-Path (Get-Location) "chaptera-reader.open-with-double-activation.json"),

    [ValidateRange(15, 600)]
    [int]$TimeoutSeconds = 120
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

function Get-NormalizedPath([string]$Path) {
    return [System.IO.Path]::GetFullPath((Resolve-Path -LiteralPath $Path).Path)
}

function Get-Sha256([string]$Path) {
    return (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
}

function Get-ReaderProcesses([string]$ExpectedExe) {
    $expected = [System.IO.Path]::GetFullPath($ExpectedExe)
    return @(
        Get-CimInstance Win32_Process -Filter "Name='chaptera-reader.exe'" -ErrorAction SilentlyContinue |
            Where-Object {
                $_.ExecutablePath -and
                [string]::Equals(
                    [System.IO.Path]::GetFullPath($_.ExecutablePath),
                    $expected,
                    [System.StringComparison]::OrdinalIgnoreCase
                )
            }
    )
}

function Wait-ReaderForSource(
    [string]$ExpectedExe,
    [string]$SourcePath,
    [System.Collections.Generic.HashSet[uint32]]$IgnoredPids,
    [int]$TimeoutSeconds
) {
    $deadline = [DateTimeOffset]::UtcNow.AddSeconds($TimeoutSeconds)
    while ([DateTimeOffset]::UtcNow -lt $deadline) {
        foreach ($candidate in (Get-ReaderProcesses $ExpectedExe)) {
            $pid = [uint32]$candidate.ProcessId
            if ($IgnoredPids.Contains($pid)) {
                continue
            }
            $commandLine = [string]$candidate.CommandLine
            if ($commandLine.IndexOf($SourcePath, [System.StringComparison]::OrdinalIgnoreCase) -ge 0) {
                return $candidate
            }
        }
        Start-Sleep -Milliseconds 250
    }
    throw "Timed out waiting for installed Chaptera Reader activation for the selected source."
}

function Wait-ProcessExit([uint32]$Pid, [int]$TimeoutSeconds) {
    $deadline = [DateTimeOffset]::UtcNow.AddSeconds($TimeoutSeconds)
    while ([DateTimeOffset]::UtcNow -lt $deadline) {
        if (-not (Get-Process -Id $Pid -ErrorAction SilentlyContinue)) {
            return
        }
        Start-Sleep -Milliseconds 250
    }
    throw "Process $Pid did not exit within $TimeoutSeconds seconds."
}

function Close-ReaderWindow([uint32]$Pid, [int]$TimeoutSeconds) {
    $process = Get-Process -Id $Pid -ErrorAction Stop
    $deadline = [DateTimeOffset]::UtcNow.AddSeconds(10)
    while ($process.MainWindowHandle -eq 0 -and [DateTimeOffset]::UtcNow -lt $deadline) {
        Start-Sleep -Milliseconds 250
        $process.Refresh()
    }
    if ($process.MainWindowHandle -eq 0) {
        throw "Reader process $Pid has no observable main window; physical Open With receipt requires the real GUI."
    }
    if (-not $process.CloseMainWindow()) {
        throw "Could not request normal close for Reader process $Pid."
    }
    Wait-ProcessExit -Pid $Pid -TimeoutSeconds $TimeoutSeconds
}

$sourceAPath = Get-NormalizedPath $SourceA
$sourceBPath = Get-NormalizedPath $SourceB
$readerPath = Get-NormalizedPath $ReaderExe
$outputPath = [System.IO.Path]::GetFullPath($Output)

if ([System.IO.Path]::GetExtension($sourceAPath) -ne ".pub" -or
    [System.IO.Path]::GetExtension($sourceBPath) -ne ".pub") {
    throw "Both sources must be .pub files."
}
if ([string]::Equals($sourceAPath, $sourceBPath, [System.StringComparison]::OrdinalIgnoreCase)) {
    throw "Use two distinct PUB files for the physical double-activation receipt."
}

$progIdCommandKey = "Registry::HKEY_CURRENT_USER\Software\Classes\Chaptera.PUB.Reader\shell\open\command"
$appCommandKey = "Registry::HKEY_CURRENT_USER\Software\Classes\Applications\chaptera-reader.exe\shell\open\command"
foreach ($key in @($progIdCommandKey, $appCommandKey)) {
    if (-not (Test-Path $key)) {
        throw "Installed Reader Open With registry command is missing: $key"
    }
}

$progIdCommand = [string](Get-Item $progIdCommandKey).GetValue("")
$appCommand = [string](Get-Item $appCommandKey).GetValue("")
foreach ($command in @($progIdCommand, $appCommand)) {
    if ($command.IndexOf($readerPath, [System.StringComparison]::OrdinalIgnoreCase) -lt 0) {
        throw "Open With command does not point at the installed current Reader: $command"
    }
    if ($command -notmatch '"%1"') {
        throw "Open With command does not quote the source path placeholder: $command"
    }
}

$sourceABefore = Get-Sha256 $sourceAPath
$sourceBBefore = Get-Sha256 $sourceBPath
$readerHash = Get-Sha256 $readerPath
$sourceABytes = (Get-Item -LiteralPath $sourceAPath).Length
$sourceBBytes = (Get-Item -LiteralPath $sourceBPath).Length

$ignored = [System.Collections.Generic.HashSet[uint32]]::new()
foreach ($process in (Get-ReaderProcesses $readerPath)) {
    [void]$ignored.Add([uint32]$process.ProcessId)
}

Write-Host ""
Write-Host "Physical Open With acceptance"
Write-Host "1. In the dialog that opens, select Chaptera PUB Reader for Source A."
Write-Host "2. After the first Reader window appears, a second dialog will open for Source B."
Write-Host "3. Select Chaptera PUB Reader again. Do not close either Reader until instructed."
Write-Host ""

Start-Process -FilePath "$env:WINDIR\System32\rundll32.exe" -ArgumentList @(
    "shell32.dll,OpenAs_RunDLL",
    ('"{0}"' -f $sourceAPath)
) | Out-Null
$processA = Wait-ReaderForSource -ExpectedExe $readerPath -SourcePath $sourceAPath -IgnoredPids $ignored -TimeoutSeconds $TimeoutSeconds
[void]$ignored.Add([uint32]$processA.ProcessId)

Start-Process -FilePath "$env:WINDIR\System32\rundll32.exe" -ArgumentList @(
    "shell32.dll,OpenAs_RunDLL",
    ('"{0}"' -f $sourceBPath)
) | Out-Null
$processB = Wait-ReaderForSource -ExpectedExe $readerPath -SourcePath $sourceBPath -IgnoredPids $ignored -TimeoutSeconds $TimeoutSeconds

$pidA = [uint32]$processA.ProcessId
$pidB = [uint32]$processB.ProcessId
if ($pidA -eq $pidB) {
    throw "Two physical Open With activations reused one Reader PID."
}

Write-Host "Observed independent Reader PIDs: A=$pidA B=$pidB"
Write-Host "Closing Reader A normally; Reader B must remain alive."
Close-ReaderWindow -Pid $pidA -TimeoutSeconds $TimeoutSeconds

Start-Sleep -Milliseconds 750
if (-not (Get-Process -Id $pidB -ErrorAction SilentlyContinue)) {
    throw "Closing Reader A terminated Reader B."
}

$sourceAAfter = Get-Sha256 $sourceAPath
$sourceBAfter = Get-Sha256 $sourceBPath
if ($sourceABefore -ne $sourceAAfter -or $sourceBBefore -ne $sourceBAfter) {
    throw "Physical Open With activation mutated a source PUB."
}

$receipt = [ordered]@{
    schema_version = "chaptera.reader-open-with-double-activation.v1"
    observed_at_utc = [DateTimeOffset]::UtcNow.ToString("o")
    reader = [ordered]@{
        sha256 = $readerHash
        installed_current_path_verified = $true
        progid_command_verified = $true
        applications_command_verified = $true
    }
    source_a = [ordered]@{
        sha256 = $sourceABefore
        byte_len = $sourceABytes
    }
    source_b = [ordered]@{
        sha256 = $sourceBBefore
        byte_len = $sourceBBytes
    }
    activation = [ordered]@{
        mechanism = "windows_open_with_dialog"
        distinct_source_distinct_pid = $true
        pid_a = $pidA
        pid_b = $pidB
        closing_a_preserves_b = $true
        source_immutable = $true
        singleton_or_ipc_broker_observed = $false
    }
    hosted_semantics_dependency = [ordered]@{
        issue = 20
        merged_pr = 190
        merge_commit = "7b2a7f58829f33a9cb4884c4386561c7779847ca"
    }
}

$outputDir = Split-Path -Parent $outputPath
if ($outputDir -and -not (Test-Path $outputDir)) {
    New-Item -ItemType Directory -Force -Path $outputDir | Out-Null
}
$receipt | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath $outputPath -Encoding utf8

Write-Host "Physical Open With receipt written to $outputPath"
Write-Host "Reader B remains open for manual inspection. Close it normally when finished."
