$ErrorActionPreference='Stop'
$base=Split-Path $PSScriptRoot -Parent
try{
 $env:OFFLINE_LOOP_DIAGNOSTIC_CLEAN_STOP='1'
 & (Join-Path $PSScriptRoot 'loop-test.ps1') -Fixture (Join-Path $base 'work\sample.avi')
}finally{Remove-Item Env:\OFFLINE_LOOP_DIAGNOSTIC_CLEAN_STOP -ErrorAction SilentlyContinue}
