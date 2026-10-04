param([switch]$SelfTest)
Set-StrictMode -Version Latest
$ErrorActionPreference='Stop'
$Root=$PSScriptRoot
$P0Zip=Join-Path $Root 'payload\PUB2019-PORTABLE-P0-V1.zip'
$ExpectedP0Sha='601c8c8ade4e6462a8ebebcc36a74a482479e33e36b3320abe66a6ae1c4eb1b1'
$ExpectedFixtureSha='5bf6057b8b11c8ee4a421d93885ae6e9e7c03a7a42d541e6d0df33497c08c33b'
$ExpectedFixtureBytes=81408
$ExpectedPublisherFileVersion='16.0.12527.22145'
$ExpectedPublisherExeSha='e1ef8811b85b82045f37c4173b92726101be3a25e550b0dcb9f178df834ab20b'
$PbFilePublication=1

function Get-Sha256([string]$Path){return (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()}
function Release-ComObject($Object){if($null -ne $Object -and [Runtime.InteropServices.Marshal]::IsComObject($Object)){try{[void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($Object)}catch{}}}
function Get-MspubPids{@((Get-Process -Name MSPUB -ErrorAction SilentlyContinue)|ForEach-Object{[int]$_.Id})}
function Ensure-PublisherPreflightIdle{
    $processes=@(Get-Process -Name MSPUB -ErrorAction SilentlyContinue);if($processes.Count -eq 0){return}
    $app=$null;try{$app=[Runtime.InteropServices.Marshal]::GetActiveObject('Publisher.Application')}catch{$app=$null}
    if($null -eq $app){throw 'MSPUB.EXE is running but no safely controllable Publisher COM session was found. Close Publisher and rerun.'}
    try{$n=[int]$app.Documents.Count;if($n -gt 0){throw "Publisher has $n open document(s). Save/close them and rerun."};$app.Quit()}finally{Release-ComObject $app}
    for($i=0;$i -lt 40;$i++){Start-Sleep -Milliseconds 250;if(@(Get-Process -Name MSPUB -ErrorAction SilentlyContinue).Count -eq 0){return}}
    throw 'Publisher did not exit after safe empty-session Quit.'
}
function Assert-NotElevated{$id=[Security.Principal.WindowsIdentity]::GetCurrent();$p=New-Object Security.Principal.WindowsPrincipal($id);if($p.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)){throw 'Do not run this bundle as Administrator.'}}
function Assert-Payload{if(-not(Test-Path -LiteralPath $P0Zip -PathType Leaf)){throw 'P0 payload missing.'};$sha=Get-Sha256 $P0Zip;if($sha -ne $ExpectedP0Sha){throw "P0 payload SHA mismatch: $sha"}}
function Assert-P0Manifest([string]$P0Root){$m=Get-Content -LiteralPath (Join-Path $P0Root 'bundle-manifest.json') -Raw|ConvertFrom-Json;foreach($item in @($m.files)){$p=Join-Path $P0Root (([string]$item.path).Replace('/','\'));if(-not(Test-Path -LiteralPath $p -PathType Leaf)){throw "P0 file missing: $($item.path)"};if((Get-Item -LiteralPath $p).Length -ne [int64]$item.bytes){throw "P0 size mismatch: $($item.path)"};if((Get-Sha256 $p) -ne [string]$item.sha256){throw "P0 SHA mismatch: $($item.path)"}};return $m}
function Assert-Publisher([string]$P0Root){
    Import-Module (Join-Path $P0Root 'tools\windows\pub-runtime\PubRuntime.psm1') -Force
    $pub=Get-PubPublisherIdentity
    if(-not $pub.available -or $pub.path.state -ne 'value'){throw 'Publisher identity unavailable.'}
    $exe=Join-Path ([string]$pub.path.value) 'MSPUB.EXE'
    if(-not(Test-Path -LiteralPath $exe -PathType Leaf)){throw 'MSPUB.EXE not found at COM-reported path.'}
    $fv=[Diagnostics.FileVersionInfo]::GetVersionInfo($exe).FileVersion;$sha=Get-Sha256 $exe
    if([string]$fv -ne $ExpectedPublisherFileVersion -or $sha -ne $ExpectedPublisherExeSha){throw "Publisher binary mismatch: version=$fv sha=$sha"}
    return [ordered]@{com_version=$(if($pub.version.state -eq 'value'){[string]$pub.version.value}else{$null});com_build=$(if($pub.build.state -eq 'value'){[string]$pub.build.value}else{$null});file_version=[string]$fv;exe_sha256=$sha;exe_path=$exe}
}
function Add-UniqueCandidate([System.Collections.Generic.List[string]]$List,[string]$Path){if([string]::IsNullOrWhiteSpace($Path)){return};try{$full=[IO.Path]::GetFullPath($Path)}catch{return};if(-not $List.Contains($full)){$List.Add($full)}}
function Find-ExactFixture([string]$DiagPath){
    $candidates=New-Object 'System.Collections.Generic.List[string]'
    if(-not [string]::IsNullOrWhiteSpace([string]$env:PUB_RESEARCH_FIXTURE)){Add-UniqueCandidate $candidates ([string]$env:PUB_RESEARCH_FIXTURE)}
    if(-not [string]::IsNullOrWhiteSpace([string]$env:PUB_RESEARCH_FIXTURE_ROOT)){Add-UniqueCandidate $candidates (Join-Path ([string]$env:PUB_RESEARCH_FIXTURE_ROOT) 'pubgen-create-20260923\minimal-blank-v1-generated.pub')}
    foreach($p in @(
        'D:\Downloads\Downloads\pubtool-0.2.0\realtest\pubgen-create-20260923\minimal-blank-v1-generated.pub',
        'D:\Downloads\pubtool-0.2.0\realtest\pubgen-create-20260923\minimal-blank-v1-generated.pub',
        (Join-Path $env:USERPROFILE 'Downloads\pubtool-0.2.0\realtest\pubgen-create-20260923\minimal-blank-v1-generated.pub'),
        (Join-Path $env:USERPROFILE 'Downloads\Downloads\pubtool-0.2.0\realtest\pubgen-create-20260923\minimal-blank-v1-generated.pub')
    )){Add-UniqueCandidate $candidates $p}
    $roots=@('D:\Downloads\Downloads','D:\Downloads',(Join-Path $env:USERPROFILE 'Downloads'))|Where-Object{Test-Path -LiteralPath $_ -PathType Container}|Select-Object -Unique
    foreach($searchRoot in $roots){
        Write-Host "Searching exact filename under $searchRoot ..."
        try{foreach($f in @(Get-ChildItem -LiteralPath $searchRoot -Filter 'minimal-blank-v1-generated.pub' -File -Recurse -ErrorAction SilentlyContinue)){Add-UniqueCandidate $candidates $f.FullName}}catch{}
    }
    $tested=0;$wrong=0
    foreach($p in $candidates){
        if(-not(Test-Path -LiteralPath $p -PathType Leaf)){continue};$tested++
        try{$fi=Get-Item -LiteralPath $p;$sha=Get-Sha256 $p;if($fi.Length -eq $ExpectedFixtureBytes -and $sha -eq $ExpectedFixtureSha){
            [ordered]@{status='found';path=$p;sha256=$sha;bytes=[int64]$fi.Length;candidates_tested=$tested;wrong_hash_candidates=$wrong}|ConvertTo-Json -Depth 5|Set-Content -LiteralPath $DiagPath -Encoding UTF8
            return $p
        }else{$wrong++}}catch{$wrong++}
    }
    [ordered]@{status='not_found';expected_sha256=$ExpectedFixtureSha;expected_bytes=$ExpectedFixtureBytes;candidates_tested=$tested;wrong_hash_candidates=$wrong}|ConvertTo-Json -Depth 5|Set-Content -LiteralPath $DiagPath -Encoding UTF8
    return $null
}
function Quote-PsLiteral([string]$Value){return "'"+$Value.Replace("'","''")+"'"}
function Start-EncodedPowerShell([string]$Script,[string[]]$Args,[string]$Stdout,[string]$Stderr){
    $parts=@('&',(Quote-PsLiteral $Script));foreach($a in $Args){$parts+=(Quote-PsLiteral $a)};$command=$parts -join ' ';$encoded=[Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($command));$argLine='-NoLogo -NoProfile -ExecutionPolicy Bypass -EncodedCommand '+$encoded
    return Start-Process -FilePath (Join-Path $env:SystemRoot 'System32\WindowsPowerShell\v1.0\powershell.exe') -ArgumentList $argLine -NoNewWindow -PassThru -RedirectStandardOutput $Stdout -RedirectStandardError $Stderr
}
function Invoke-Step([string]$Label,[string]$Script,[string[]]$Args,[int]$TimeoutSeconds,[string]$LogDir,[string]$LiveLog,[string]$WatchRoot){
    New-Item -ItemType Directory -Force -Path $LogDir|Out-Null;$stdout=Join-Path $LogDir 'stdout.txt';$stderr=Join-Path $LogDir 'stderr.txt';$start=Get-Date
    Write-Host '';Write-Host ('=== '+$Label+' ===') -ForegroundColor Cyan
    $p=Start-EncodedPowerShell $Script $Args $stdout $stderr
    while(-not $p.HasExited){
        Start-Sleep -Seconds 10;$p.Refresh();$elapsed=[int]((Get-Date)-$start).TotalSeconds;$mp=@(Get-MspubPids)-join ',';$files=0;if(Test-Path -LiteralPath $WatchRoot){$files=@(Get-ChildItem -LiteralPath $WatchRoot -File -Recurse -ErrorAction SilentlyContinue).Count}
        $line=('['+(Get-Date -Format 'HH:mm:ss')+"] $Label RUNNING elapsed=${elapsed}s child=$($p.Id) MSPUB=[$mp] evidence_files=$files");Write-Host $line;$line|Add-Content -LiteralPath $LiveLog -Encoding UTF8
        foreach($lp in @($stdout,$stderr)){if(Test-Path -LiteralPath $lp){$tail=Get-Content -LiteralPath $lp -Tail 3 -ErrorAction SilentlyContinue;foreach($t in $tail){if(-not [string]::IsNullOrWhiteSpace($t)){Write-Host ('  > '+$t)}}}}
        if($elapsed -ge $TimeoutSeconds){try{Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue}catch{};foreach($id in @(Get-MspubPids)){try{Stop-Process -Id $id -Force -ErrorAction SilentlyContinue}catch{}};throw "$Label timed out after $TimeoutSeconds s"}
    }
    if([int]$p.ExitCode -ne 0){throw "$Label failed with exit code $($p.ExitCode)"}
}
function Copy-SafeTree([string]$SourceRoot,[string]$DestRoot){New-Item -ItemType Directory -Force -Path $DestRoot|Out-Null;foreach($name in @('analysis','logs')){$src=Join-Path $SourceRoot $name;if(Test-Path -LiteralPath $src){Copy-Item -LiteralPath $src -Destination $DestRoot -Recurse -Force}};foreach($name in @('environment.json','evidence-manifest.json')){$src=Join-Path $SourceRoot $name;if(Test-Path -LiteralPath $src -PathType Leaf){Copy-Item -LiteralPath $src -Destination (Join-Path $DestRoot $name) -Force}}}
function Invoke-SelfTest{
    $base=Join-Path $env:TEMP ('paragraph-v12-selftest-'+[guid]::NewGuid().ToString('N'));New-Item -ItemType Directory -Force -Path $base|Out-Null
    try{
        $fake=Join-Path $base 'fake child.ps1'
        @(
            'param([string]$A,[string]$B)',
            "if(`$A -ne 'path with spaces' -or `$B -ne 'second value'){exit 7}",
            'exit 0'
        )|Set-Content -LiteralPath $fake -Encoding ASCII
        $o=Join-Path $base 'o.txt';$e=Join-Path $base 'e.txt'
        $p=Start-EncodedPowerShell $fake @('path with spaces','second value') $o $e
        $p.WaitForExit()
        if($p.ExitCode -ne 0){throw 'encoded child invocation self-test failed'}
        Write-Host 'SELFTEST PASS'
    }finally{Remove-Item -LiteralPath $base -Recurse -Force -ErrorAction SilentlyContinue}
}
if($SelfTest){Invoke-SelfTest;exit 0}

$stamp=Get-Date -Format 'yyyyMMdd-HHmmss';$WorkRoot=Join-Path $Root ('work\'+$stamp);$ReturnRoot=Join-Path $Root ('return\'+$stamp);$LiveDir=Join-Path $Root 'live';New-Item -ItemType Directory -Force -Path $WorkRoot,$ReturnRoot,$LiveDir|Out-Null;$LiveLog=Join-Path $LiveDir ('LIVE-CONSOLE-'+$stamp+'.log')
$summary=[ordered]@{schema='chaptera.paragraph-exact-fixture-v12.v1';started_at=(Get-Date).ToString('o');publisher=$null;fixture=$null;status='running'}
try{
    Assert-NotElevated;Assert-Payload;Ensure-PublisherPreflightIdle
    $P0Root=Join-Path $WorkRoot 'p0';Expand-Archive -LiteralPath $P0Zip -DestinationPath $P0Root -Force;[void](Assert-P0Manifest $P0Root);$summary.publisher=Assert-Publisher $P0Root
    $recovery=Join-Path $ReturnRoot 'FIXTURE-RECOVERY.json';$fixture=Find-ExactFixture $recovery
    if([string]::IsNullOrWhiteSpace($fixture)){throw 'Exact canonical paragraph fixture SHA 5bf6057b... was not found on this machine.'}
    $fixtureCopy=Join-Path $WorkRoot 'fixture\minimal-blank-v1-generated.pub';New-Item -ItemType Directory -Force -Path (Split-Path -Parent $fixtureCopy)|Out-Null;Copy-Item -LiteralPath $fixture -Destination $fixtureCopy -Force
    if((Get-Sha256 $fixtureCopy) -ne $ExpectedFixtureSha){throw 'Copied fixture SHA mismatch.'};$summary.fixture=[ordered]@{sha256=$ExpectedFixtureSha;bytes=$ExpectedFixtureBytes;provenance='recovered_exact_local_5bf6057b'}
    $env:PUB_RESEARCH_FIXTURE=$fixtureCopy
    $Out=Join-Path $WorkRoot 'paragraph-metrics';New-Item -ItemType Directory -Force -Path $Out|Out-Null
    $packet=Join-Path $P0Root 'tools\research-runner\experiments\paragraph-metrics-auth-01.packet.json';$operation=Join-Path $P0Root 'tools\research-runner\operations\paragraph_metrics_auth_01.ps1'
    Invoke-Step 'PARAGRAPH NATIVE MATRIX' $operation @('-PacketPath',$packet,'-OutputRoot',$Out) 900 (Join-Path $ReturnRoot 'native-console') $LiveLog $Out
    [ordered]@{schema='chaptera.paragraph-exact-fixture-environment.v1';publisher=$summary.publisher;fixture=$summary.fixture}|ConvertTo-Json -Depth 10|Set-Content -LiteralPath (Join-Path $Out 'environment.json') -Encoding UTF8
    $pythonRoot=Join-Path $WorkRoot 'python';Expand-Archive -LiteralPath (Join-Path $P0Root 'runtime\python-3.13.16-embed-amd64.zip') -DestinationPath $pythonRoot -Force;$python=Join-Path $pythonRoot 'python.exe'
    $blast=Join-Path $P0Root 'tools\research-runner\analysis\paragraph_metrics_auth_01_blast_radius.py';$struct=Join-Path $P0Root 'tools\research-runner\analysis\paragraph_metrics_auth_01_structural.py';$probe=Join-Path $P0Root 'runtime\paragraph-metrics-probe.exe'
    & $python $blast --output-root $Out;if($LASTEXITCODE -ne 0){throw 'blast-radius analysis failed'}
    & $python $struct --output-root $Out --snapshot-tool $probe;if($LASTEXITCODE -ne 0){throw 'structural analysis failed'}
    $final=Join-Path $P0Root 'tools\research-runner\finalize_native_run.ps1';Invoke-Step 'PARAGRAPH FINALIZE' $final @('-PacketPath',$packet,'-OutputRoot',$Out) 180 (Join-Path $ReturnRoot 'finalize-console') $LiveLog $Out
    $required=@('analysis\paragraph-metrics-auth-01.json','analysis\paragraph-metrics-auth-01-blast-radius.json','analysis\paragraph-metrics-auth-01-structural.json','environment.json','evidence-manifest.json','logs\paragraph-metrics-auth-01.txt');foreach($rel in $required){if(-not(Test-Path -LiteralPath (Join-Path $Out $rel) -PathType Leaf)){throw "required evidence missing: $rel"}}
    $native=Get-Content -LiteralPath (Join-Path $Out 'analysis\paragraph-metrics-auth-01.json') -Raw|ConvertFrom-Json;if([string]$native.verdict -ne 'native-semantic-arms-captured-with-common-seed'){throw "unexpected native verdict: $($native.verdict)"};if(@($native.arms).Count -ne 11){throw 'expected 11 native arms'}
    $summary.status='success';Copy-SafeTree $Out (Join-Path $ReturnRoot 'paragraph-metrics')
}catch{$summary.status='failed';$summary.error=$_.Exception.Message}
$summary.finished_at=(Get-Date).ToString('o');$summary|ConvertTo-Json -Depth 15|Set-Content -LiteralPath (Join-Path $ReturnRoot 'SUMMARY.json') -Encoding UTF8;if(Test-Path -LiteralPath $LiveLog){Copy-Item -LiteralPath $LiveLog -Destination (Join-Path $ReturnRoot 'LIVE-RUN.log') -Force}
$zip=Join-Path $Root ('RETURN-TO-CHAT-PARAGRAPH-'+$stamp+'.zip');if(Test-Path -LiteralPath $zip){Remove-Item -LiteralPath $zip -Force};Compress-Archive -Path (Join-Path $ReturnRoot '*') -DestinationPath $zip -CompressionLevel Optimal
Write-Host '';Write-Host ('FINAL STATUS: '+$summary.status);Write-Host ('RETURN ZIP: '+$zip) -ForegroundColor Green
if($summary.status -eq 'success'){exit 0}else{exit 2}
