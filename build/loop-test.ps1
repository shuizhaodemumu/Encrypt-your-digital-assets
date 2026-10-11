param([string]$Fixture)
$ErrorActionPreference='Stop'
$taskRoot=Split-Path $PSScriptRoot -Parent
if(-not $Fixture){$Fixture=Join-Path (Split-Path $taskRoot -Parent) 'work\sample.mp4'}
if(-not(Test-Path -LiteralPath $Fixture)){throw 'Provide a short MP4 with -Fixture; fixtures are not uploaded.'}
$env:OFFLINE_LOOP_FIXTURE=[IO.Path]::GetFullPath($Fixture)
$taskSdk=$taskRoot
while(-not(Test-Path -LiteralPath (Join-Path $taskSdk 'toolchain\cargo\bin\cargo.exe'))){
    $taskAncestor=Split-Path $taskSdk -Parent
    if(-not $taskAncestor -or $taskAncestor -eq $taskSdk){break}
    $taskSdk=$taskAncestor
}
$taskCargo=Join-Path $taskSdk 'toolchain\cargo\bin\cargo.exe'
if(Test-Path -LiteralPath $taskCargo){
    $env:RUSTUP_HOME=Join-Path $taskSdk 'toolchain\rustup'
    $env:CARGO_HOME=Join-Path $taskRoot 'toolchain\cargo'
}else{$taskCargo=(Get-Command cargo.exe -ErrorAction Stop).Source}
$env:CARGO_TARGET_DIR=Join-Path $taskRoot 'target'
$taskLlvm=Join-Path $taskSdk 'toolchain\llvm-mingw\llvm-mingw-20261006-ucrt-x86_64\bin'
if(Test-Path -LiteralPath $taskLlvm){
    $env:PATH="$taskLlvm;$env:PATH"
    $env:CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER=Join-Path $taskLlvm 'x86_64-w64-mingw32-clang.exe'
    $env:RUSTFLAGS='-C link-self-contained=yes'
}
$taskOutput=& $taskCargo test --manifest-path (Join-Path $taskRoot 'Cargo.toml') --locked encrypted_video_repeats_without_rebuilding_output -- --ignored --nocapture --test-threads=1 2>&1
$taskExitCode=$LASTEXITCODE
foreach($taskLine in $taskOutput){
    if([string]$taskLine -match 'Using D3D11VA'){Write-Output 'VLC hardware decoding active (device details omitted).'}else{Write-Output ([string]$taskLine)}
}
if($taskExitCode){throw 'Native encrypted-video loop regression failed'}
