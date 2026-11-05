param([switch]$PowerPoint)
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'toolchain.ps1')
$cargo=Initialize-ProjectToolchain (Split-Path $PSScriptRoot -Parent)
if($PowerPoint){& $cargo test --locked --features watchdog-test-mode real_powerpoint_ -- --ignored --nocapture --test-threads=1}else{& $cargo test --locked --features watchdog-test-mode}
if($LASTEXITCODE){throw 'Tests failed'}
