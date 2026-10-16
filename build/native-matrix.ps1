$ErrorActionPreference='Stop'
$base=Split-Path $PSScriptRoot -Parent
foreach($ext in @('mp4','mkv','avi','mov','webm')){
 Write-Output ('NATIVE_FORMAT_BEGIN '+$ext)
 & (Join-Path $PSScriptRoot 'loop-test.ps1') -Fixture (Join-Path $base ('work\sample.'+$ext))
 Write-Output ('NATIVE_FORMAT_PASS '+$ext)
}
