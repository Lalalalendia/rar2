param([switch]$SelfTest)
Set-StrictMode -Version Latest
$ErrorActionPreference='Stop'
$Root=$PSScriptRoot
$P0Zip=Join-Path $Root 'payload\PUB2019-PORTABLE-P0-V1.zip'
$QuillZip=Join-Path $Root 'payload\QUILL-STORY-READONLY-ORACLE-PORTABLE.zip'
$ExpectedP0Sha='601c8c8ade4e6462a8ebebcc36a74a482479e33e36b3320abe66a6ae1c4eb1b1'
$ExpectedQuillSha='9de1e3f8b4006854918dd9de4e4b62644268224f010de3638e78ccb5dbc0b7e4'
$ExpectedPublisherVersion='16.0';$ExpectedPublisherBuild='12527';$ExpectedPublisherFileVersion='16.0.12527.22145';$ExpectedPublisherExeSha='e1ef8811b85b82045f37c4173b92726101be3a25e550b0dcb9f178df834ab20b'

function Get-Sha256([string]$Path){return (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()}
function Release-Com($Value){if($null -ne $Value -and [Runtime.InteropServices.Marshal]::IsComObject($Value)){try{[void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($Value)}catch{}}}
function Get-MspubPids{@((Get-Process -Name MSPUB -ErrorAction SilentlyContinue)|ForEach-Object{[int]$_.Id})}
function Cleanup-Mspub{$processIds=@(Get-MspubPids);if($processIds.Count -eq 0){return @()};Start-Sleep -Seconds 2;$processIds=@(Get-MspubPids);if($processIds.Count -gt 0){foreach($processId in $processIds){try{Stop-Process -Id $processId -Force -ErrorAction Stop}catch{}};Start-Sleep -Milliseconds 500};return @($processIds)}
function Ensure-PublisherPreflightIdle{
    $processes=@(Get-Process -Name MSPUB -ErrorAction SilentlyContinue);if($processes.Count -eq 0){return}
    $app=$null;try{$app=[Runtime.InteropServices.Marshal]::GetActiveObject('Publisher.Application')}catch{$app=$null}
    if($null -eq $app){throw 'MSPUB.EXE is already running but no safely controllable active Publisher COM session was found. Close Publisher and rerun.'}
    try{$documentCount=[int]$app.Documents.Count;if($documentCount -gt 0){throw "Publisher is already running with $documentCount open document(s). Save/close them and rerun."};Write-Host 'Publisher preflight: empty session detected; closing it safely.';$app.Quit()}finally{Release-Com $app}
    for($i=0;$i -lt 40;$i++){Start-Sleep -Milliseconds 250;if(@(Get-Process -Name MSPUB -ErrorAction SilentlyContinue).Count -eq 0){return}}
    throw 'Publisher preflight requested Quit, but MSPUB.EXE is still running.'
}
function Assert-NotElevated{$identity=[Security.Principal.WindowsIdentity]::GetCurrent();$principal=New-Object Security.Principal.WindowsPrincipal($identity);if($principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)){throw 'Do not run this bundle as Administrator.'}}
function Assert-Payload([string]$Path,[string]$ExpectedSha){if(-not(Test-Path -LiteralPath $Path -PathType Leaf)){throw "Payload missing: $Path"};$actual=Get-Sha256 $Path;if($actual -ne $ExpectedSha){throw "Payload SHA mismatch: $Path expected $ExpectedSha got $actual"}}
function Assert-P0Manifest([string]$P0Root){$manifest=Get-Content -LiteralPath (Join-Path $P0Root 'bundle-manifest.json') -Raw|ConvertFrom-Json;foreach($item in @($manifest.files)){$relative=([string]$item.path).Replace('/','\');$candidate=Join-Path $P0Root $relative;if(-not(Test-Path -LiteralPath $candidate -PathType Leaf)){throw "P0 file missing: $($item.path)"};if((Get-Item -LiteralPath $candidate).Length -ne [int64]$item.bytes){throw "P0 size mismatch: $($item.path)"};if((Get-Sha256 $candidate) -ne [string]$item.sha256){throw "P0 SHA mismatch: $($item.path)"}};return $manifest}
function Assert-QuillManifest([string]$QuillRoot){$manifest=Get-Content -LiteralPath (Join-Path $QuillRoot 'BUNDLE-MANIFEST.json') -Raw|ConvertFrom-Json;foreach($item in @($manifest.files)){$relative=([string]$item.path).Replace('/','\');$candidate=Join-Path $QuillRoot $relative;if(-not(Test-Path -LiteralPath $candidate -PathType Leaf)){throw "Quill file missing: $($item.path)"};if((Get-Item -LiteralPath $candidate).Length -ne [int64]$item.size){throw "Quill size mismatch: $($item.path)"};if((Get-Sha256 $candidate) -ne [string]$item.sha256){throw "Quill SHA mismatch: $($item.path)"}};return $manifest}
function Assert-Publisher([string]$P0Root){
    Import-Module (Join-Path $P0Root 'tools\windows\pub-runtime\PubRuntime.psm1') -Force
    $attempts=@()
    for($attempt=1;$attempt -le 3;$attempt++){
        $publisher=Get-PubPublisherIdentity
        $diag=[ordered]@{
            attempt=$attempt
            available=[bool]$publisher.available
            version_state=$(if($publisher.available){[string]$publisher.version.state}else{$null})
            version_value=$(if($publisher.available -and $publisher.version.state -eq 'value'){[string]$publisher.version.value}else{$null})
            build_state=$(if($publisher.available){[string]$publisher.build.state}else{$null})
            build_value=$(if($publisher.available -and $publisher.build.state -eq 'value'){[string]$publisher.build.value}else{$null})
            name_value=$(if($publisher.available -and $publisher.name.state -eq 'value'){[string]$publisher.name.value}else{$null})
            path_value=$(if($publisher.available -and $publisher.path.state -eq 'value'){[string]$publisher.path.value}else{$null})
            file_version=$null
            exe_sha256=$null
            exact_binary=$false
        }
        if($publisher.available -and $publisher.path.state -eq 'value'){
            $exe=Join-Path ([string]$publisher.path.value) 'MSPUB.EXE'
            if(Test-Path -LiteralPath $exe -PathType Leaf){
                $fv=[Diagnostics.FileVersionInfo]::GetVersionInfo($exe).FileVersion
                $sha=Get-Sha256 $exe
                $diag.file_version=[string]$fv
                $diag.exe_sha256=$sha
                $diag.exact_binary=([string]$fv -eq $ExpectedPublisherFileVersion -and $sha -eq $ExpectedPublisherExeSha)
                $attempts += $diag
                if($diag.exact_binary){
                    return [ordered]@{
                        authority='exact_mspub_binary'
                        expected_version=$ExpectedPublisherVersion
                        expected_build=$ExpectedPublisherBuild
                        com_version=$diag.version_value
                        com_build=$diag.build_value
                        com_version_match=($diag.version_value -eq $ExpectedPublisherVersion)
                        com_build_match=($diag.build_value -eq $ExpectedPublisherBuild)
                        name=$diag.name_value
                        file_version=$diag.file_version
                        exe_sha256=$diag.exe_sha256
                        exe_path=$exe
                        attempts=@($attempts)
                    }
                }
            } else { $attempts += $diag }
        } else { $attempts += $diag }
        Start-Sleep -Seconds 2
    }
    throw ('Exact Publisher binary identity not proven after 3 attempts. Diagnostics: '+($attempts|ConvertTo-Json -Compress -Depth 8))
}
function Copy-SafeTask([string]$SourceRoot,[string]$DestRoot){New-Item -ItemType Directory -Force -Path $DestRoot|Out-Null;foreach($name in @('analysis','logs','return')){$src=Join-Path $SourceRoot $name;if(Test-Path -LiteralPath $src){Copy-Item -LiteralPath $src -Destination $DestRoot -Recurse -Force}};foreach($name in @('environment.json','environment-portable.json','evidence-manifest.json','task-summary.json')){$src=Join-Path $SourceRoot $name;if(Test-Path -LiteralPath $src -PathType Leaf){Copy-Item -LiteralPath $src -Destination (Join-Path $DestRoot $name) -Force}}}
function Quote-PsLiteral([string]$Value){return "'" + $Value.Replace("'","''") + "'"}
function New-EncodedTaskCommand([string]$Script,[hashtable]$Parameters){
    $parts=@('&',(Quote-PsLiteral $Script))
    foreach($key in @($Parameters.Keys|Sort-Object)){$parts+=('-'+[string]$key);$parts+=(Quote-PsLiteral ([string]$Parameters[$key]))}
    return ($parts -join ' ')
}
function Test-TaskEvidence([string]$WorkDir,[string[]]$RequiredEvidence){
    $summaryPath=Join-Path $WorkDir 'task-summary.json'
    if(-not(Test-Path -LiteralPath $summaryPath -PathType Leaf)){return 'task-summary.json missing'}
    try{$taskSummary=Get-Content -LiteralPath $summaryPath -Raw|ConvertFrom-Json}catch{return ('task-summary.json parse failed: '+$_.Exception.Message)}
    if([string]$taskSummary.status -ne 'success'){return ('task-summary status is '+[string]$taskSummary.status)}
    foreach($relative in $RequiredEvidence){$candidate=Join-Path $WorkDir $relative;if(-not(Test-Path -LiteralPath $candidate -PathType Leaf)){return ('required evidence missing: '+$relative)}}
    return $null
}
function Invoke-Task([string]$Id,[string]$Label,[string]$Script,[hashtable]$Parameters,[string[]]$RequiredEvidence,[int]$TimeoutSeconds,[string]$WorkDir,[string]$ReturnDir){
    Write-Host '';Write-Host ('=== '+$Label+' ===') -ForegroundColor Cyan
    New-Item -ItemType Directory -Force -Path $WorkDir,$ReturnDir|Out-Null
    $stdout=Join-Path $ReturnDir 'runner-stdout.txt';$stderr=Join-Path $ReturnDir 'runner-stderr.txt';$started=Get-Date;$timedOut=$false;$exitCode=900;$validationError=$null;$evidenceValidated=$false
    try{
        $command=New-EncodedTaskCommand -Script $Script -Parameters $Parameters
        $encoded=[Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($command))
        $argLine='-NoLogo -NoProfile -ExecutionPolicy Bypass -EncodedCommand '+$encoded
        $child=Start-Process -FilePath (Join-Path $env:SystemRoot 'System32\WindowsPowerShell\v1.0\powershell.exe') -ArgumentList $argLine -NoNewWindow -PassThru -RedirectStandardOutput $stdout -RedirectStandardError $stderr
        $done=$child.WaitForExit($TimeoutSeconds*1000)
        if(-not $done){$timedOut=$true;try{Stop-Process -Id $child.Id -Force -ErrorAction SilentlyContinue}catch{};$exitCode=124}else{$exitCode=[int]$child.ExitCode}
    }catch{$_.Exception.ToString()|Set-Content -LiteralPath (Join-Path $ReturnDir 'runner-launch-error.txt') -Encoding UTF8;$exitCode=900}
    $cleaned=@(Cleanup-Mspub)
    if(-not $timedOut -and $exitCode -eq 0){$validationError=Test-TaskEvidence -WorkDir $WorkDir -RequiredEvidence $RequiredEvidence;if([string]::IsNullOrWhiteSpace($validationError)){$evidenceValidated=$true}else{$validationError|Set-Content -LiteralPath (Join-Path $ReturnDir 'runner-validation-error.txt') -Encoding UTF8;$exitCode=65}}
    Copy-SafeTask $WorkDir $ReturnDir
    $ended=Get-Date;$status=if($timedOut){'TIMEOUT'}elseif($exitCode -eq 0 -and $evidenceValidated){'PASS'}else{'FAIL'};Write-Host ('  '+$status+' (exit '+$exitCode+', evidence='+$evidenceValidated+')')
    return [ordered]@{id=$Id;label=$Label;status=$status;exit_code=$exitCode;evidence_validated=$evidenceValidated;validation_error=$validationError;timed_out=$timedOut;timeout_seconds=$TimeoutSeconds;duration_seconds=[Math]::Round(($ended-$started).TotalSeconds,3);forced_cleanup_pids=@($cleaned)}
}
function Invoke-SelfTest{
    $base=Join-Path $env:TEMP ('chaptera batch self test '+[guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Force -Path $base|Out-Null
    try{
        $successScript=Join-Path $base 'fake success.ps1';$failScript=Join-Path $base 'fake fail.ps1';$falsePassScript=Join-Path $base 'fake false pass.ps1'
        @'
param([string]$P0Root,[string]$OutputRoot)
New-Item -ItemType Directory -Force -Path (Join-Path $OutputRoot 'analysis')|Out-Null
@{status='success'}|ConvertTo-Json|Set-Content -LiteralPath (Join-Path $OutputRoot 'task-summary.json') -Encoding UTF8
'ok'|Set-Content -LiteralPath (Join-Path $OutputRoot 'analysis\required.txt') -Encoding ASCII
exit 0
'@|Set-Content -LiteralPath $successScript -Encoding ASCII
        @'
param([string]$P0Root,[string]$OutputRoot)
New-Item -ItemType Directory -Force -Path $OutputRoot|Out-Null
@{status='failed';error='intentional'}|ConvertTo-Json|Set-Content -LiteralPath (Join-Path $OutputRoot 'task-summary.json') -Encoding UTF8
Write-Error 'intentional failure'
exit 1
'@|Set-Content -LiteralPath $failScript -Encoding ASCII
        @'
param([string]$P0Root,[string]$OutputRoot)
New-Item -ItemType Directory -Force -Path $OutputRoot|Out-Null
exit 0
'@|Set-Content -LiteralPath $falsePassScript -Encoding ASCII
        $payloadRoot=Join-Path $base 'payload root with spaces';New-Item -ItemType Directory -Force -Path $payloadRoot|Out-Null
        $a=Invoke-Task 'smoke-success' 'smoke success' $successScript @{P0Root=$payloadRoot;OutputRoot=(Join-Path $base 'work success')} @('analysis\required.txt') 30 (Join-Path $base 'work success') (Join-Path $base 'return success')
        if($a.status -ne 'PASS' -or -not $a.evidence_validated){throw 'Self-test success task did not PASS with validated evidence.'}
        $b=Invoke-Task 'smoke-fail' 'smoke fail' $failScript @{P0Root=$payloadRoot;OutputRoot=(Join-Path $base 'work fail')} @() 30 (Join-Path $base 'work fail') (Join-Path $base 'return fail')
        if($b.status -ne 'FAIL' -or $b.exit_code -eq 0){throw 'Self-test failing task was not classified FAIL.'}
        $c=Invoke-Task 'smoke-false-pass' 'smoke false pass' $falsePassScript @{P0Root=$payloadRoot;OutputRoot=(Join-Path $base 'work false pass')} @('analysis\required.txt') 30 (Join-Path $base 'work false pass') (Join-Path $base 'return false pass')
        if($c.status -ne 'FAIL' -or $c.exit_code -ne 65){throw 'Self-test missing-evidence task was not converted to FAIL/65.'}
        Write-Host 'SELFTEST PASS'
    }finally{Remove-Item -LiteralPath $base -Recurse -Force -ErrorAction SilentlyContinue}
}

if($SelfTest){Invoke-SelfTest;exit 0}

$stamp=Get-Date -Format 'yyyyMMdd-HHmmss';$WorkRoot=Join-Path $Root ('work\'+$stamp);$ReturnRoot=Join-Path $Root ('return\'+$stamp);New-Item -ItemType Directory -Force -Path $WorkRoot,$ReturnRoot|Out-Null
$summary=[ordered]@{schema='chaptera.publisher-native-canonical-batch.v3';started_at=(Get-Date).ToString('o');payloads=[ordered]@{p0_sha256=$ExpectedP0Sha;quill_sha256=$ExpectedQuillSha};publisher=$null;tasks=@();final_status='running'}
try{
    Assert-NotElevated;Assert-Payload $P0Zip $ExpectedP0Sha;Assert-Payload $QuillZip $ExpectedQuillSha;Ensure-PublisherPreflightIdle
    $p0Root=Join-Path $WorkRoot 'p0';$quillRoot=Join-Path $WorkRoot 'quill';Expand-Archive -LiteralPath $P0Zip -DestinationPath $p0Root -Force;Expand-Archive -LiteralPath $QuillZip -DestinationPath $quillRoot -Force
    [void](Assert-P0Manifest $p0Root);[void](Assert-QuillManifest $quillRoot);$summary.publisher=Assert-Publisher $p0Root
    $paragraphWork=Join-Path $WorkRoot 'paragraph';$summary.tasks += Invoke-Task 'paragraph' 'PARAGRAPH METRICS - canonical portable path' (Join-Path $Root 'tasks\paragraph-canonical.ps1') @{P0Root=$p0Root;OutputRoot=$paragraphWork} @('analysis\paragraph-metrics-auth-01.json','analysis\paragraph-metrics-auth-01-blast-radius.json','analysis\paragraph-metrics-auth-01-structural.json','evidence-manifest.json') 600 $paragraphWork (Join-Path $ReturnRoot 'paragraph')
    $quillWork=Join-Path $WorkRoot 'quill-output';$summary.tasks += Invoke-Task 'quill' 'EARLY QUILL - official portable oracle' (Join-Path $Root 'tasks\quill-canonical.ps1') @{QuillRoot=$quillRoot;OutputRoot=$quillWork} @('analysis\quill-story-readonly-oracle-resolution.json','analysis\quill-story-readonly-oracle.json') 600 $quillWork (Join-Path $ReturnRoot 'quill')
    $surfaceWork=Join-Path $WorkRoot '029';$summary.tasks += Invoke-Task '029' 'MATURE 029 - canonical native oracle' (Join-Path $Root 'tasks\029-canonical.ps1') @{P0Root=$p0Root;OutputRoot=$surfaceWork} @('analysis\mature-029-native-page-spread-oracle-01.json','analysis\mature-029-reference-surface-classification.json') 300 $surfaceWork (Join-Path $ReturnRoot '029')
    $passed=@($summary.tasks|Where-Object{$_.status -eq 'PASS'}).Count;$summary.final_status=if($passed -eq 3){'all_three_pass'}elseif($passed -gt 0){'partial_success'}else{'failed'}
}catch{$summary.final_status='bootstrap_failed';$summary.bootstrap_error=$_.Exception.Message}
$summary.finished_at=(Get-Date).ToString('o');$summary|ConvertTo-Json -Depth 16|Set-Content -LiteralPath (Join-Path $ReturnRoot 'BATCH-SUMMARY.json') -Encoding UTF8
$zip=Join-Path $Root ('RETURN-TO-CHAT-'+$stamp+'.zip');if(Test-Path -LiteralPath $zip){Remove-Item -LiteralPath $zip -Force};Compress-Archive -Path (Join-Path $ReturnRoot '*') -DestinationPath $zip -CompressionLevel Optimal
Write-Host '';Write-Host ('FINAL STATUS: '+$summary.final_status);Write-Host ('RETURN ZIP: '+$zip) -ForegroundColor Green
if($summary.final_status -eq 'all_three_pass'){exit 0}else{exit 2}
