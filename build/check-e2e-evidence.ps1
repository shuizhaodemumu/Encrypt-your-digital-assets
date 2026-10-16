$ErrorActionPreference='Stop'
$base=Split-Path $PSScriptRoot -Parent
$text=Get-Content -Raw -LiteralPath (Join-Path $base 'work\video-e2e-final.log')
foreach($ext in @('MP4','MKV','AVI','MOV','WEBM')){
 foreach($suffix in @('_SAME_PID_THREE_NATIVE_LOOPS_FULLSCREEN_HIDDEN_PASS','_REGISTERED_PHYSICAL_STARTUP_FRESH_ENV_AUTH_RESUME_PASS')){
  if(!$text.Contains($ext+$suffix)){throw ('Missing final actual GUI evidence: '+$ext+$suffix)}
  Write-Output ($ext+$suffix)
 }
}
foreach($marker in @('PAUSE_11_SECONDS_RESUME_ESC_F11_DOCK_SHOW_HIDE_PASS','ALL_VIDEO_E2E_PASS_NO_REAL_REBOOT_OR_LOGOFF')){
 if(!$text.Contains($marker)){throw ('Missing '+$marker)};Write-Output $marker
}
if($text -match 'Exception:|Three video loops missing'){throw 'Final GUI run contains failure'}
Write-Output 'Evidence review only: no actual reboot/login test and no independent Gemini GUI operation claimed.'
