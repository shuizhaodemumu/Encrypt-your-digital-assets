$ErrorActionPreference='Stop'
$base=Split-Path $PSScriptRoot -Parent
$sdk=$base
while(!(Test-Path -LiteralPath (Join-Path $sdk 'toolchain\cargo\bin\cargo.exe'))){
 $next=Split-Path $sdk -Parent
 if(!$next -or $next -eq $sdk){throw 'Rust SDK not found'}
 $sdk=$next
}
$env:RUSTUP_HOME=Join-Path $sdk 'toolchain\rustup'
$env:CARGO_HOME=Join-Path $base 'toolchain\cargo'
$env:CARGO_TARGET_DIR=Join-Path $base 'target'
$llvm=Join-Path $sdk 'toolchain\llvm-mingw\llvm-mingw-20261006-ucrt-x86_64\bin'
$env:PATH="$llvm;$env:PATH"
$env:CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER=Join-Path $llvm 'x86_64-w64-mingw32-clang.exe'
$env:RUSTFLAGS='-C link-self-contained=yes'
& (Join-Path $sdk 'toolchain\cargo\bin\cargo.exe') test --manifest-path (Join-Path $base 'Cargo.toml') --locked
if($LASTEXITCODE){throw 'Tests failed'}
