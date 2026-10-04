Set-StrictMode -Version Latest
$ErrorActionPreference='Stop'

function Quote-PsLiteral([string]$Value){ return "'" + $Value.Replace("'","''") + "'" }

function Start-EncodedPowerShell([string]$Script,[string[]]$ChildArgs,[string]$Stdout,[string]$Stderr){
    $parts=@('&',(Quote-PsLiteral $Script))
    foreach($a in $ChildArgs){
        if($a -match '^-{1,2}[A-Za-z][A-Za-z0-9_-]*$'){$parts += $a}else{$parts += (Quote-PsLiteral $a)}
    }
    $call = $parts -join ' '
    $command = '$ErrorActionPreference=''Stop''; try { ' + $call + '; if(-not $?) { exit 1 }; exit 0 } catch { Write-Error $_; exit 1 }'
    $encoded=[Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($command))
    $argLine='-NoLogo -NoProfile -ExecutionPolicy Bypass -EncodedCommand '+$encoded
    return Start-Process -FilePath (Join-Path $env:SystemRoot 'System32\WindowsPowerShell\v1.0\powershell.exe') -ArgumentList $argLine -NoNewWindow -PassThru -RedirectStandardOutput $Stdout -RedirectStandardError $Stderr
}

$base=Join-Path $env:TEMP ('paragraph-v13-helper-'+[guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Force -Path $base | Out-Null
try {
    $fake=Join-Path $base 'fake child.ps1'
    @(
        'param([string]$PacketPath,[string]$OutputRoot)',
        "if(`$PacketPath -ne 'path with spaces' -or `$OutputRoot -ne 'second value'){exit 7}",
        'exit 0'
    ) | Set-Content -LiteralPath $fake -Encoding ASCII

    $o=Join-Path $base 'o.txt'; $e=Join-Path $base 'e.txt'
    $p=Start-EncodedPowerShell $fake @('-PacketPath','path with spaces','-OutputRoot','second value') $o $e
    $p.WaitForExit(); $p.Refresh(); $exitCode=[int]$p.ExitCode
    if($exitCode -ne 0){ throw "named binding failed: exit=$exitCode stderr=$(Get-Content -LiteralPath $e -Raw -ErrorAction SilentlyContinue)" }

    $o2=Join-Path $base 'o2.txt'; $e2=Join-Path $base 'e2.txt'
    $p2=Start-EncodedPowerShell $fake @('-NoSuchParameter','x') $o2 $e2
    $p2.WaitForExit(); $p2.Refresh(); $badExit=[int]$p2.ExitCode
    if($badExit -eq 0){ throw 'invalid named parameter incorrectly returned success' }

    Write-Host 'HELPER SELFTEST PASS'
} finally {
    Remove-Item -LiteralPath $base -Recurse -Force -ErrorAction SilentlyContinue
}
