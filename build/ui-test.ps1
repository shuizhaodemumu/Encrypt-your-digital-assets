param([switch]$SkipBuild)
$ErrorActionPreference='Stop'
$taskRoot=Split-Path $PSScriptRoot -Parent
. (Join-Path $PSScriptRoot 'toolchain.ps1')
$taskCargo=Initialize-ProjectToolchain $taskRoot
if(-not $SkipBuild){
    & $taskCargo build --manifest-path (Join-Path $taskRoot 'Cargo.toml') --locked --features watchdog-test-mode
    if($LASTEXITCODE){throw 'UI test build failed'}
}
& (Join-Path $PSScriptRoot 'ui-language.ps1') -Executable (Join-Path $env:CARGO_TARGET_DIR 'debug\offline-license-rust.exe')
