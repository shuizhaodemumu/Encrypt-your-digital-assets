$ErrorActionPreference='Stop'
function Initialize-ProjectToolchain {
 param([string]$ProjectRoot)
 $candidates=@($ProjectRoot,(Join-Path $ProjectRoot 'toolchain'),$env:OFFLINE_RUST_SDK)
 foreach($candidate in $candidates){
  if(!$candidate){continue}
  foreach($sdk in @($candidate,(Join-Path $candidate 'toolchain'))){
   if(!(Test-Path -LiteralPath (Join-Path $sdk 'cargo\bin\cargo.exe')) -or !(Test-Path -LiteralPath (Join-Path $sdk 'rustup'))){continue}
   $env:RUSTUP_HOME=Join-Path $sdk 'rustup'
   $env:CARGO_HOME=Join-Path $ProjectRoot 'toolchain\cargo'
   $env:CARGO_TARGET_DIR=Join-Path $ProjectRoot 'target'
   $llvm=Join-Path $sdk 'llvm-mingw\llvm-mingw-20261006-ucrt-x86_64\bin'
   $gcc=Join-Path $sdk 'mingw\mingw64\bin'
   if(Test-Path -LiteralPath (Join-Path $llvm 'x86_64-w64-mingw32-clang.exe')){
    $env:PATH="$llvm;$env:PATH";$env:CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER=Join-Path $llvm 'x86_64-w64-mingw32-clang.exe'
   }elseif(Test-Path -LiteralPath (Join-Path $gcc 'gcc.exe')){
    $env:PATH="$gcc;$env:PATH";$env:CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER=Join-Path $gcc 'gcc.exe'
   }else{continue}
   $env:RUSTFLAGS='-C link-self-contained=yes'
   $env:PYTHONIOENCODING='utf-8'
   [Console]::OutputEncoding=[Text.UTF8Encoding]::new($false)
   return (Join-Path $sdk 'cargo\bin\cargo.exe')
  }
 }
 $systemCargo=Get-Command cargo.exe -ErrorAction SilentlyContinue
 if($systemCargo){
  $env:CARGO_TARGET_DIR=Join-Path $ProjectRoot 'target'
  return $systemCargo.Source
 }
 throw 'Rust SDK not found. Install Rust for Windows or set OFFLINE_RUST_SDK to an existing SDK root.'
}
