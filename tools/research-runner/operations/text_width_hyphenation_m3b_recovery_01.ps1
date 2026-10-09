param(
    [Parameter(Mandatory=$true)][ValidateSet("verify","restore")][string]$Mode,
    [Parameter(Mandatory=$true)][string]$OutputRoot
)
Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"
$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot "../../..")).Path
Import-Module (Join-Path $RepoRoot "tools/windows/pub-runtime/PubRuntime.psm1") -Force
$AnalysisDir = Join-Path $OutputRoot "analysis"
New-Item -ItemType Directory -Force -Path $AnalysisDir | Out-Null
$Marker = Join-Path $env:USERPROFILE ".chaptera-publisher-hyphenation-quarantine"
$Stage = Join-Path $AnalysisDir ("text-width-m3b-" + $Mode + ".json")
$app = $null
$options = $null
try {
    if (-not (Test-Path -LiteralPath $Marker -PathType Leaf) -or
        (Get-Content -LiteralPath $Marker -Raw).Trim() -cne "TEXT-WIDTH-HYPHENATION-M3B-01") {
        throw "m3b_restore_marker_missing_or_wrong"
    }
    $app = New-PubPublisherApplication -AllowHyphenationRecovery
    $options = $app.Options
    $beforeAuto = [bool]$options.AutoHyphenate
    $beforeZone = [double]$options.HyphenationZone
    if ($Mode -eq "restore") {
        $options.AutoHyphenate = $true
        $options.HyphenationZone = 18.0
    }
    $afterAuto = [bool]$options.AutoHyphenate
    $afterZone = [double]$options.HyphenationZone
    if (-not $afterAuto -or [math]::Abs($afterZone - 18.0) -gt 0.000001) {
        throw "m3b_independent_restore_verification_failed"
    }
    Write-PubJson -Path $Stage -Value ([ordered]@{
        schema = "chaptera.text-width-m3b-recovery.v1"
        experiment_id = "TEXT-WIDTH-HYPHENATION-M3B-01"
        action = $Mode
        settings_before = [ordered]@{auto_hyphenate=$beforeAuto;hyphenation_zone_pt=$beforeZone}
        settings_after = [ordered]@{auto_hyphenate=$afterAuto;hyphenation_zone_pt=$afterZone}
        verified = $true
        source_bytes_uploaded = $false
    })
} catch {
    Write-PubJson -Path $Stage -Value ([ordered]@{
        schema = "chaptera.text-width-m3b-recovery.v1"
        experiment_id = "TEXT-WIDTH-HYPHENATION-M3B-01"
        action = $Mode
        verified = $false
        hresult_hex = ('0x{0:X8}' -f (([long]$_.Exception.HResult) -band 4294967295L))
        source_bytes_uploaded = $false
    })
    throw
} finally {
    if ($null -ne $options -and [Runtime.InteropServices.Marshal]::IsComObject($options)) {
        try { [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($options) } catch {}
    }
    Close-PubPublisherApplication $app
}
