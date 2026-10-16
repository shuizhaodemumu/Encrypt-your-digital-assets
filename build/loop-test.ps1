param([string]$Fixture)
$ErrorActionPreference='Stop'
$taskRoot=Split-Path $PSScriptRoot -Parent
if(-not $Fixture){$Fixture=Join-Path $taskRoot 'work\sample.mp4'}
if(-not(Test-Path -LiteralPath $Fixture)){throw 'Provide a short MP4 with -Fixture; fixtures are not uploaded.'}
$env:OFFLINE_LOOP_FIXTURE=[IO.Path]::GetFullPath($Fixture)
. (Join-Path $PSScriptRoot 'toolchain.ps1')
$taskCargo=Initialize-ProjectToolchain $taskRoot
$taskOutput=& $taskCargo test --manifest-path (Join-Path $taskRoot 'Cargo.toml') --locked encrypted_video_repeats_without_rebuilding_output -- --ignored --nocapture --test-threads=1 2>&1
$taskExitCode=$LASTEXITCODE
foreach($taskLine in $taskOutput){
    if([string]$taskLine -match 'Using D3D11VA'){Write-Output 'VLC hardware decoding active (device details omitted).'}else{Write-Output ([string]$taskLine)}
}
if($taskExitCode){throw 'Native encrypted-video loop regression failed'}
