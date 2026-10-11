param([switch]$SkipBuild)
$ErrorActionPreference='Stop'
$taskRoot=Split-Path $PSScriptRoot -Parent
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
if(-not $SkipBuild){
    & $taskCargo build --manifest-path (Join-Path $taskRoot 'Cargo.toml') --locked --features watchdog-test-mode
    if($LASTEXITCODE){throw 'UI test build failed'}
}
& (Join-Path $PSScriptRoot 'ui-language.ps1') -Executable (Join-Path $env:CARGO_TARGET_DIR 'debug\offline-license-rust.exe')
