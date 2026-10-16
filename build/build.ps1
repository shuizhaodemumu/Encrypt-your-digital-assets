param([switch]$Release,[switch]$OfficeCompat)
$ErrorActionPreference='Stop'
$base=Split-Path $PSScriptRoot -Parent
. (Join-Path $PSScriptRoot 'toolchain.ps1')
$cargo=Initialize-ProjectToolchain $base
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
 else{
  foreach($name in @('Packager.exe','DeviceTool.exe','Packager-Setup.exe')){
   try {Copy-Item -LiteralPath "$base\target\release\offline-license-rust.exe" -Destination (Join-Path $out $name) -Force -ErrorAction Stop}
   catch [System.IO.IOException] {
    $version=(Select-String -LiteralPath "$base\Cargo.toml" -Pattern '^version = "([^"]+)"').Matches[0].Groups[1].Value
    $staged=Join-Path $out (([IO.Path]::GetFileNameWithoutExtension($name))+"-$version.exe")
    Copy-Item -LiteralPath "$base\target\release\offline-license-rust.exe" -Destination $staged -Force
    Write-Warning "The running program could not be replaced. Close it and use $staged."
   }
  }
 }
}
