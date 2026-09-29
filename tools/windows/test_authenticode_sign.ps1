param(
  [Parameter(Mandatory = $true, Position = 0)]
  [string]$File
)

$ErrorActionPreference = "Stop"

$signtool = $env:CHAPTERA_SIGNTOOL_PATH
$thumbprint = $env:CHAPTERA_TEST_SIGN_THUMBPRINT
if (-not $signtool) { throw "CHAPTERA_SIGNTOOL_PATH is not set" }
if (-not $thumbprint) { throw "CHAPTERA_TEST_SIGN_THUMBPRINT is not set" }
if (-not (Test-Path -LiteralPath $signtool)) { throw "signtool.exe not found: $signtool" }
if (-not (Test-Path -LiteralPath $File)) { throw "signing target missing: $File" }

& $signtool sign /sha1 $thumbprint /fd SHA256 $File
if ($LASTEXITCODE -ne 0) { throw "signtool failed with exit code $LASTEXITCODE for $File" }

$savedErrorActionPreference = $ErrorActionPreference
$ErrorActionPreference = "Continue"
try {
  $verifyOutput = @(& $signtool verify /pa /all /v $File 2>&1)
  $verifyExit = $LASTEXITCODE
} finally {
  $ErrorActionPreference = $savedErrorActionPreference
}
$verifyText = $verifyOutput -join [Environment]::NewLine
$verifyOutput | ForEach-Object { Write-Output $_ }

if ($verifyExit -ne 0) {
  $expectedUntrustedRoot = $verifyText -match "(?is)certificate chain processed.*terminated in a root\s+certificate which is not trusted by the trust provider"
  $exactlyOneError = $verifyText -match "(?im)^Number of errors:\s*1\s*$"
  if (-not ($expectedUntrustedRoot -and $exactlyOneError)) {
    throw "signtool integrity verification failed with unexpected error for ${File}: exit=$verifyExit"
  }
}
