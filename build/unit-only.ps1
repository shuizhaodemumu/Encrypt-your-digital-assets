$ErrorActionPreference='Stop'
$base=Split-Path $PSScriptRoot -Parent
. (Join-Path $PSScriptRoot 'toolchain.ps1')
$cargo=Initialize-ProjectToolchain $base
& $cargo test --manifest-path (Join-Path $base 'Cargo.toml') --locked
if($LASTEXITCODE){throw 'Tests failed'}
