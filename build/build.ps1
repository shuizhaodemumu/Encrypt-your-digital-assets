param([switch]$Release,[switch]$OfficeCompat)
$ErrorActionPreference='Stop'
$base=Split-Path $PSScriptRoot -Parent
$sdkBase=$base
while(!(Test-Path -LiteralPath (Join-Path $sdkBase 'toolchain\cargo\bin\cargo.exe'))){
 $ancestor=Split-Path $sdkBase -Parent
 if(!$ancestor -or $ancestor -eq $sdkBase){break}
 $sdkBase=$ancestor
}
$localCargo=Join-Path $sdkBase 'toolchain\cargo\bin\cargo.exe'
$env:CARGO_TARGET_DIR=Join-Path $base 'target'
if(Test-Path -LiteralPath $localCargo){
 $env:RUSTUP_HOME=Join-Path $sdkBase 'toolchain\rustup'
 $env:CARGO_HOME=Join-Path $base 'toolchain\cargo'
 $cargo=$localCargo
}else{
 $cargo=(Get-Command cargo.exe -ErrorAction Stop).Source
}
$llvm=Join-Path $sdkBase 'toolchain\llvm-mingw\llvm-mingw-20261006-ucrt-x86_64\bin'
if(Test-Path -LiteralPath $llvm){
 $env:PATH="$llvm;$env:PATH"
 $env:CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER=Join-Path $llvm 'x86_64-w64-mingw32-clang.exe'
 $env:RUSTFLAGS='-C link-self-contained=yes'
}
& $cargo fmt --manifest-path "$base\Cargo.toml"
if($LASTEXITCODE){throw 'Formatting failed'}
$taskCargoFlags=if($OfficeCompat){@('--features','office-state-compat')}else{@()}
& $cargo test --manifest-path "$base\Cargo.toml" --locked @taskCargoFlags
if($LASTEXITCODE){throw 'Tests failed'}
if($Release){
 & $cargo build --manifest-path "$base\Cargo.toml" --locked --release @taskCargoFlags
 if($LASTEXITCODE){throw 'Release build failed'}
 $out=Join-Path $base '程序'
 New-Item -ItemType Directory -Path $out -Force | Out-Null
 if($OfficeCompat){Copy-Item -LiteralPath "$base\target\release\offline-license-rust.exe" -Destination (Join-Path $out 'ViewerUpdater-OfficeCompat.exe') -Force}
 else{foreach($name in @('Packager.exe','DeviceTool.exe','Packager-Setup.exe')){Copy-Item -LiteralPath "$base\target\release\offline-license-rust.exe" -Destination (Join-Path $out $name) -Force}}
}
