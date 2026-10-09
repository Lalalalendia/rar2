param(
    [Parameter(Mandatory = $true)][string]$InputPath,
    [Parameter(Mandatory = $true)][string]$OutputRoot
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$ExpectedSha256 = "082319ac283c99f1b2e5d05fe428bccfd4123ef498267b35b492bba991a3b6e0"
$ExpectedBytes = [int64]2202112
$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot "../..")).Path
$Probe = Join-Path $RepoRoot "tools/windows/run_publisher_supplemental_native_surface_probe.ps1"

$resolvedInput = (Resolve-Path -LiteralPath $InputPath).Path
$source = Get-Item -LiteralPath $resolvedInput
if ([int64]$source.Length -ne $ExpectedBytes) {
    throw "075 source size mismatch: expected $ExpectedBytes got $($source.Length)"
}
$sha = (Get-FileHash -LiteralPath $resolvedInput -Algorithm SHA256).Hash.ToLowerInvariant()
if ($sha -ne $ExpectedSha256) {
    throw "075 source SHA mismatch: expected $ExpectedSha256 got $sha"
}

if (-not [System.IO.Path]::IsPathRooted($OutputRoot)) {
    $OutputRoot = Join-Path $RepoRoot $OutputRoot
}
if (Test-Path -LiteralPath $OutputRoot) {
    if (@(Get-ChildItem -LiteralPath $OutputRoot -Force).Count -ne 0) {
        throw "OutputRoot must be empty: $OutputRoot"
    }
} else {
    New-Item -ItemType Directory -Force -Path $OutputRoot | Out-Null
}

$receipt = Join-Path $OutputRoot "publisher-075-surface-stage.json"
& pwsh -NoProfile -File $Probe `
    -InputPath $resolvedInput `
    -ExpectedSha256 $ExpectedSha256 `
    -ExpectedBytes $ExpectedBytes `
    -OutputPath $receipt
if ($LASTEXITCODE -ne 0) {
    throw "Publisher 075 surface-stage probe failed with exit code $LASTEXITCODE"
}
if (-not (Test-Path -LiteralPath $receipt -PathType Leaf)) {
    throw "Publisher 075 surface-stage receipt missing"
}

$doc = Get-Content -LiteralPath $receipt -Raw | ConvertFrom-Json
if ([string]$doc.schema -ne "chaptera.publisher-supplemental-native-surface.v1") {
    throw "Unexpected 075 native receipt schema"
}
if ([string]$doc.source.sha256 -ne $ExpectedSha256 -or -not [bool]$doc.source.unchanged_after_probe) {
    throw "075 native receipt does not preserve exact source identity"
}

$repoSha = (& git -C $RepoRoot rev-parse HEAD).Trim()
if ($LASTEXITCODE -ne 0 -or $repoSha -notmatch '^[0-9a-f]{40}$') {
    throw "Unable to bind repository HEAD for native return"
}
$probeBlob = (& git -C $RepoRoot hash-object -- "tools/windows/run_publisher_supplemental_native_surface_probe.ps1").Trim()
if ($LASTEXITCODE -ne 0 -or $probeBlob -notmatch '^[0-9a-f]{40}$') {
    throw "Unable to bind existing surface probe blob"
}

$metadata = [ordered]@{
    schema = "chaptera.publisher-075-surface-stage-return.v1"
    repo_sha = $repoSha
    probe_git_blob = $probeBlob
    source_sha256 = $ExpectedSha256
    source_bytes = $ExpectedBytes
    receipt_file = "publisher-075-surface-stage.json"
    claims = [ordered]@{
        source_bytes_in_return = $false
        opened_read_only = $true
        exact_source_identity_bound = $true
    }
}
$metadataPath = Join-Path $OutputRoot "publisher-075-surface-stage-return.json"
$metadata | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $metadataPath -Encoding utf8

$zip = Join-Path $OutputRoot "publisher-075-surface-stage-return.zip"
Compress-Archive -LiteralPath @($receipt, $metadataPath) -DestinationPath $zip -CompressionLevel Optimal

Write-Host "PUBLISHER_075_SURFACE_RETURN=$zip"
Get-Content -LiteralPath $receipt -Raw
