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

$signature = Get-AuthenticodeSignature -LiteralPath $File
if (-not $signature.SignerCertificate) { throw "no signer certificate after signing: $File" }
if ($signature.SignerCertificate.Thumbprint -ne $thumbprint) {
  throw "unexpected signer thumbprint for $File: $($signature.SignerCertificate.Thumbprint)"
}
