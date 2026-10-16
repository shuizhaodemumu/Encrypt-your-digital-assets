$ErrorActionPreference='Stop'
$base=Split-Path $PSScriptRoot -Parent
& (Join-Path $PSScriptRoot 'check-e2e-evidence.ps1')
$controls=Get-Content -Raw -LiteralPath (Join-Path $base 'work\video-controls-final.log')
foreach($marker in @('MP4_SAME_PID_THREE_NATIVE_LOOPS_FULLSCREEN_HIDDEN_PASS','PAUSE_11_SECONDS_RESUME_ESC_F11_DOCK_SHOW_HIDE_PASS','MP4_REGISTERED_PHYSICAL_STARTUP_FRESH_ENV_AUTH_RESUME_PASS','ALL_VIDEO_E2E_PASS_NO_REAL_REBOOT_OR_LOGOFF')){
 if(!$controls.Contains($marker)){throw ('Missing final UI replay: '+$marker)}
}
if($controls -match 'Exception:'){throw 'Latest controls run failed'}
$script=Get-Content -Raw -LiteralPath (Join-Path $PSScriptRoot 'video-e2e.ps1')
if(!$script.Contains('Window mode parent was hidden') -or !$script.Contains('Window mode controls were hidden')){throw 'Visibility assertions were not added'}
Write-Output 'FINAL_WINDOW_VISIBLE_CONTROLS_VISIBLE_FULLSCREEN_DOCK_REVEAL_HIDE_REPLAY_PASS'
Write-Output 'Read-only review of Codex actual GUI evidence; not independent GUI clicking.'
