param(
    [Parameter(Mandatory = $true)]
    [string]$Destination
)

$ErrorActionPreference = "Stop"

$destinationPath = [System.IO.Path]::GetFullPath($Destination)
New-Item -ItemType Directory -Force -Path $destinationPath | Out-Null

$workerTarget = [System.IO.Path]::GetFullPath("target/desktop-open-worker-runtime")
$sandboxTarget = [System.IO.Path]::GetFullPath("target/desktop-open-sandbox-runtime")
$previousRustflags = $env:RUSTFLAGS

try {
    $env:RUSTFLAGS = "-C target-feature=+crt-static"

    cargo build --manifest-path "apps/chaptera-desktop-open-worker/Cargo.toml" --release --bin "chaptera-desktop-open-worker" --target-dir $workerTarget
    if ($LASTEXITCODE -ne 0) {
        throw "contained desktop-open worker build failed: $LASTEXITCODE"
    }

    cargo build --manifest-path "apps/chaptera-desktop-open-sandbox/Cargo.toml" --release --bin "chaptera-desktop-open-sandbox-host" --target-dir $sandboxTarget
    if ($LASTEXITCODE -ne 0) {
        throw "desktop-open sandbox host build failed: $LASTEXITCODE"
    }
}
finally {
    if ($null -eq $previousRustflags) {
        Remove-Item Env:RUSTFLAGS -ErrorAction SilentlyContinue
    }
    else {
        $env:RUSTFLAGS = $previousRustflags
    }
}

$worker = Join-Path $workerTarget "release/chaptera-desktop-open-worker.exe"
$host = Join-Path $sandboxTarget "release/chaptera-desktop-open-sandbox-host.exe"
foreach ($path in @($worker, $host)) {
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
        throw "desktop-open runtime binary missing: $path"
    }
    if ((Get-Item -LiteralPath $path).Length -le 0) {
        throw "desktop-open runtime binary is empty: $path"
    }
}

Copy-Item -LiteralPath $worker -Destination (Join-Path $destinationPath "chaptera-desktop-open-worker.exe") -Force
Copy-Item -LiteralPath $host -Destination (Join-Path $destinationPath "chaptera-desktop-open-sandbox-host.exe") -Force

$receipt = @{
    schema_version = "chaptera.desktop-open-runtime-stage.v1"
    worker = @{
        file_name = "chaptera-desktop-open-worker.exe"
        sha256 = (Get-FileHash -LiteralPath $worker -Algorithm SHA256).Hash.ToLowerInvariant()
        byte_len = (Get-Item -LiteralPath $worker).Length
    }
    sandbox_host = @{
        file_name = "chaptera-desktop-open-sandbox-host.exe"
        sha256 = (Get-FileHash -LiteralPath $host -Algorithm SHA256).Hash.ToLowerInvariant()
        byte_len = (Get-Item -LiteralPath $host).Length
    }
}
$receipt | ConvertTo-Json -Depth 4 | Set-Content -Encoding utf8 (Join-Path $destinationPath "chaptera-desktop-open-runtime.json")
