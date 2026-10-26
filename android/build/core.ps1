param([ValidateSet('test','android','host','fmt')][string]$Mode='test')
$ErrorActionPreference='Stop'
$root=Split-Path $PSScriptRoot -Parent
$maps=@{V='D:\codex create\驱动总裁Rust重构';Y=$root;Z='D:\codex create\2026-10-14\zui'}
$created=@()
try {
 Add-Type -TypeDefinition 'using System; using System.Text; using System.Runtime.InteropServices; public class EavDrive { [DllImport("kernel32.dll",CharSet=CharSet.Unicode)] public static extern uint QueryDosDevice(string n,StringBuilder s,int len); }' -ErrorAction SilentlyContinue
 foreach($drive in $maps.Keys){if(!(Test-Path ($drive+':\'))){subst ($drive+':') $maps[$drive];$created+=$drive};$b=[Text.StringBuilder]::new(2048);[void][EavDrive]::QueryDosDevice(($drive+':'),$b,2048);if($b.ToString() -ne ('\??\'+$maps[$drive])){throw ('盘符已被占用: '+$drive)}}
 $env:RUSTUP_HOME='V:\work\rust-tools\rustup';$env:CARGO_HOME='V:\work\rust-tools\cargo'
 $env:PATH='V:\work\rust-tools\rustup\toolchains\stable-x86_64-pc-windows-gnu\bin;V:\work\rust-tools\mingw\mingw64\bin;'+$env:PATH
 $env:CARGO_ENCODED_RUSTFLAGS=$null
 $env:CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER='Y:\build\host-linker.cmd'
 $manifest='Y:\core\Cargo.toml'
 if($Mode -eq 'android'){
  $env:RUSTFLAGS=$null
  $env:CARGO_ENCODED_RUSTFLAGS=@('-C','linker-flavor=ld.lld','-C','linker=V:\work\rust-tools\rustup\toolchains\stable-x86_64-pc-windows-gnu\lib\rustlib\x86_64-pc-windows-gnu\bin\rust-lld.exe','-L','Z:\work\android-tools\ndk\android-ndk-r27c\toolchains\llvm\prebuilt\windows-x86_64\sysroot\usr\lib\aarch64-linux-android\26','-L','Z:\work\android-tools\native-libs','-C','link-arg=-z','-C','link-arg=max-page-size=16384') -join [char]31
  cargo build --manifest-path $manifest --release --target aarch64-linux-android --lib
 }else{
  $env:RUSTFLAGS='--sysroot=V:\work\rust-tools\rustup\toolchains\stable-x86_64-pc-windows-gnu -C link-arg=-specs=V:\work\mingw-endfile.specs'
  if($Mode -eq 'test'){cargo test --manifest-path $manifest --lib --bins}
  elseif($Mode -eq 'fmt'){cargo fmt --manifest-path $manifest}
  else{cargo build --manifest-path $manifest --release --bin video-admin}
 }
 if($LASTEXITCODE -ne 0){throw 'Rust 构建/测试失败'}
}finally{foreach($drive in $created){subst ($drive+':') /D}}
