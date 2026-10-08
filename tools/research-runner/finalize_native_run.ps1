param(
    [Parameter(Mandatory = $true)]
    [string]$PacketPath,
    [Parameter(Mandatory = $true)]
    [string]$OutputRoot
)

$ErrorActionPreference = "Stop"

function Get-Sha256([string]$Path) {
    return (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
}

function Get-RelativePathCompat([string]$Root, [string]$Path) {
    $rootFull = [IO.Path]::GetFullPath($Root).TrimEnd(
        [char[]]@([IO.Path]::DirectorySeparatorChar, [IO.Path]::AltDirectorySeparatorChar)
    )
    $pathFull = [IO.Path]::GetFullPath($Path)
    $prefix = $rootFull + [IO.Path]::DirectorySeparatorChar
    if (-not $pathFull.StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase)) {
        throw "Evidence path escaped OutputRoot: $pathFull"
    }
    return $pathFull.Substring($prefix.Length).Replace("\", "/")
}

$packet = Get-Content -LiteralPath $PacketPath -Raw | ConvertFrom-Json
$root = (Resolve-Path -LiteralPath $OutputRoot).Path

$requiredEvidence = @($packet.evidence.required)
if ($null -ne $packet.reset -and [bool]$packet.reset.required) {
    $resetReceiptRelative = "analysis/reset-receipt.json"
    if ($requiredEvidence -notcontains $resetReceiptRelative) {
        $requiredEvidence += $resetReceiptRelative
    }
}

$missing = @()
foreach ($relative in $requiredEvidence) {
    $candidate = Join-Path $root ([string]$relative)
    if (-not (Test-Path -LiteralPath $candidate)) {
        $missing += [string]$relative
    }
}
if ($missing.Count -gt 0) {
    throw "Required evidence is missing: $($missing -join ', ')"
}

$forbiddenExtensions = @(
    ".pub", ".exe", ".dll", ".com", ".msi", ".msp", ".cab", ".iso",
    ".doc", ".docx", ".xls", ".xlsx", ".ppt", ".pptx"
)
foreach ($publicDirName in @("logs", "analysis")) {
    $publicDir = Join-Path $root $publicDirName
    if (Test-Path -LiteralPath $publicDir) {
        foreach ($file in Get-ChildItem -LiteralPath $publicDir -File -Recurse) {
            if ($forbiddenExtensions -contains $file.Extension.ToLowerInvariant()) {
                throw "Forbidden binary/document in upload-safe directory: $($file.FullName)"
            }
        }
    }
}

$uploadSafeFiles = @()
foreach ($publicDirName in @("logs", "analysis")) {
    $publicDir = Join-Path $root $publicDirName
    if (Test-Path -LiteralPath $publicDir) {
        $uploadSafeFiles += @(Get-ChildItem -LiteralPath $publicDir -File -Recurse)
    }
}
$environmentPath = Join-Path $root "environment.json"
if (Test-Path -LiteralPath $environmentPath -PathType Leaf) {
    $uploadSafeFiles += @(Get-Item -LiteralPath $environmentPath)
}

$sensitiveValues = @(
    $env:RUNNER_NAME,
    $env:GITHUB_WORKSPACE,
    $env:RUNNER_TEMP,
    $env:USERPROFILE,
    $env:HOME
) | Where-Object { -not [string]::IsNullOrWhiteSpace([string]$_) } | Select-Object -Unique

foreach ($file in $uploadSafeFiles) {
    if ($file.Length -gt 8MB) {
        throw "Upload-safe text evidence is unexpectedly large: $($file.FullName)"
    }
    $text = Get-Content -LiteralPath $file.FullName -Raw -ErrorAction Stop
    foreach ($value in $sensitiveValues) {
        if ($text.IndexOf([string]$value, [StringComparison]::OrdinalIgnoreCase) -ge 0) {
            throw "Upload-safe evidence contains a local runner/workspace identifier: $($file.Name)"
        }
    }
}

$records = @()
foreach ($file in Get-ChildItem -LiteralPath $root -File -Recurse | Sort-Object FullName) {
    $relative = Get-RelativePathCompat -Root $root -Path $file.FullName
    if ($relative -eq "evidence-manifest.json") { continue }
    $visibility = if (
        $relative.StartsWith("logs/") -or
        $relative.StartsWith("analysis/") -or
        $relative -eq "environment.json"
    ) {
        "upload-safe"
    } else {
        "private-local"
    }
    $records += [ordered]@{
        path = $relative
        size = $file.Length
        sha256 = Get-Sha256 $file.FullName
        visibility = $visibility
    }
}

$manifest = [ordered]@{
    schema = "pub-research-evidence.v1"
    experiment_id = [string]$packet.id
    publisher_environment = [string]$packet.publisher_environment
    generated_utc = [DateTime]::UtcNow.ToString("o")
    required = @($requiredEvidence)
    files = $records
}
$manifest | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $root "evidence-manifest.json") -Encoding UTF8
Write-Host "Evidence bundle validated: $($records.Count) files indexed."
