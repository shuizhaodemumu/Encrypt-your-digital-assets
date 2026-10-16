param([string[]]$Formats=@('mp4','mkv','avi','mov','webm'))
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'ui-common.ps1')
. (Join-Path $PSScriptRoot 'run-rust.ps1')
Add-Type -AssemblyName System.Drawing
Add-Type @'
using System;using System.Runtime.InteropServices;
public static class VlcUi {
 [DllImport("user32.dll")]public static extern bool PrintWindow(IntPtr h,IntPtr dc,uint flags);
 [DllImport("user32.dll")]public static extern bool ShowWindow(IntPtr h,int cmd);
 [DllImport("user32.dll")]public static extern bool SetProcessDPIAware();
 [DllImport("user32.dll")]public static extern IntPtr SetFocus(IntPtr h);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)]public static extern IntPtr SendMessage(IntPtr h,uint msg,IntPtr w,IntPtr l);
 [StructLayout(LayoutKind.Sequential)] public struct Rect {public int left,top,right,bottom;}
 [DllImport("user32.dll")]public static extern bool GetWindowRect(IntPtr h,out Rect r);
 [DllImport("user32.dll")]public static extern bool SetCursorPos(int x,int y);
 [DllImport("user32.dll")]public static extern bool SetForegroundWindow(IntPtr h);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)]public static extern IntPtr SendMessageTimeout(IntPtr h,uint m,IntPtr w,IntPtr l,uint f,uint t,out IntPtr v);
}
'@
[VlcUi]::SetProcessDPIAware()|Out-Null
$tool=Join-Path $base 'target\debug\offline-license-rust.exe'
$fixtureKey=Get-ChildItem -LiteralPath (Join-Path $base 'work') -Filter test.issuerkey -Recurse | Sort-Object LastWriteTime -Descending | Select-Object -First 1 -ExpandProperty FullName
if(!$fixtureKey){throw 'Run ui-test.ps1 first to create the synthetic test issuer'}
$workspace=Join-Path $base ('work\video-e2e-'+[guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $workspace|Out-Null
$pref=Join-Path $env:LOCALAPPDATA 'OfflineLicenseVlcRust\ui-language.txt'
$savedPref=if(Test-Path -LiteralPath $pref){[IO.File]::ReadAllBytes($pref)}else{$null}
[IO.File]::WriteAllText($pref,'zh')
function Wait-Until([scriptblock]$Condition,[string]$Message,[int]$Seconds=35){
 $deadline=[DateTime]::UtcNow.AddSeconds($Seconds)
 while([DateTime]::UtcNow -lt $deadline){if(&$Condition){return};Start-Sleep -Milliseconds 100}
 throw $Message
}
function Close-Video($process,$window){
 if($process -and !$process.HasExited){[SmokeWindows]::PostMessage($window,0x10,[IntPtr]::Zero,[IntPtr]::Zero)|Out-Null;if(!$process.WaitForExit(10000)){throw 'Normal close timed out'}}
}
function Screenshot($window,$name){
 [VlcUi]::SetForegroundWindow($window)|Out-Null;Start-Sleep -Milliseconds 300
 $rect=[VlcUi+Rect]::new();[VlcUi]::GetWindowRect($window,[ref]$rect)|Out-Null
 $bitmap=[Drawing.Bitmap]::new($rect.right-$rect.left,$rect.bottom-$rect.top)
 $graphics=[Drawing.Graphics]::FromImage($bitmap)
 $dc=$graphics.GetHdc()
 try{if(![VlcUi]::PrintWindow($window,$dc,2)){throw "PrintWindow capture failed"}}finally{$graphics.ReleaseHdc($dc)}
 $bitmap.Save((Join-Path $base ('work\'+$name+'.png')))
 $graphics.Dispose();$bitmap.Dispose()
}
$viewer=$null;$window=[IntPtr]::Zero;$installed=$null;$content=$null
try{
 $admin=Start-Process -FilePath $tool -WindowStyle Hidden -PassThru
 try{$adminWindow=Window $admin 'RustVlcOfflineWindow' '*';[VlcUi]::ShowWindow($adminWindow,9)|Out-Null;Screenshot $adminWindow '新版管理员界面'}finally{if(!$admin.HasExited){Close-Video $admin $adminWindow}}
 foreach($ext in $Formats){
  $content=Join-Path $workspace ('content-'+$ext+'.exe');$report=$content+'.json';$license=$content+'.lic';$meta=$content+'.olmeta'
  Run-Rust $tool @('--pack',(Join-Path $base ('work\sample.'+$ext)),$fixtureKey,$content) 'test-only-password'
  Run-Rust $tool @('--enroll',$content,$report)
  Run-Rust $tool @('--inspect',$content,($content+'.info.json'))
  $info=Get-Content -LiteralPath ($content+'.info.json') -Raw|ConvertFrom-Json
  $startupName='OfflineLicenseVlcRust-'+$info.ContentId.Replace('-','')
  $runKey='HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'
  if((Get-ItemProperty -LiteralPath $runKey -ErrorAction SilentlyContinue).PSObject.Properties.Name -contains $startupName){throw 'Startup enabled before authorization'}
  $viewer=Start-Process -FilePath $content -WindowStyle Hidden -PassThru
  $window=Window $viewer 'RustVlcOfflineWindow' '*测试构建*'
  Run-Rust $tool @('--issue',$fixtureKey,$meta,$report,'1',$license) 'test-only-password'
  Click (Button $window '导入固态链密钥文件');FileDialog $viewer $license
  Wait-Until {([SmokeWindows]::GetWindowLongPtr($window,-16).ToInt64() -band 0x00C00000) -eq 0} 'Fullscreen failed'
  # Keep pointer away from the reveal region for hidden UI checks.
  [VlcUi]::SetCursorPos(10,10)|Out-Null
  Wait-Until {[SmokeWindows]::GetProp($window,'RustLoopCount').ToInt64() -ge 3} ('Three video loops missing: '+$ext) 45
  if($viewer.HasExited){throw ('Viewer exited at EOF: '+$ext)}
  $result=[IntPtr]::Zero
  if([VlcUi]::SendMessageTimeout($window,0,[IntPtr]::Zero,[IntPtr]::Zero,2,1000,[ref]$result) -eq [IntPtr]::Zero){throw 'UI became unresponsive'}
  if([SmokeWindows]::GetProp($window,'RustLoopFallbacks').ToInt64() -ne 0){throw 'Native loop fell back to external replay'}
  if([SmokeWindows]::Children($window)|Where-Object{[SmokeWindows]::IsWindowVisible($_) -and [SmokeWindows]::Class($_) -in @('Button','msctls_trackbar32')}){throw 'Fullscreen UI was not hidden'}
  Write-Output ($ext.ToUpper()+'_SAME_PID_THREE_NATIVE_LOOPS_FULLSCREEN_HIDDEN_PASS')
  $command=(Get-ItemProperty -LiteralPath $runKey).$startupName
  if($command -notmatch '^"(.+)" --autostart$'){throw 'Startup command missing'}
  $installed=$Matches[1]
  if(!(Test-Path -LiteralPath $installed) -or $installed.StartsWith('\\?\')){throw 'Shell startup path is invalid'}
  if($ext -eq 'mp4'){
   [SmokeWindows]::PostMessage($window,0x100,[IntPtr]32,[IntPtr]::Zero)|Out-Null
   Wait-Until {[SmokeWindows]::GetProp($window,'RustMediaState').ToInt64() -eq 4} 'Space pause failed'
   Start-Sleep -Seconds 11
   if($viewer.HasExited){throw 'Paused video triggered watchdog'}
   [SmokeWindows]::PostMessage($window,0x100,[IntPtr]32,[IntPtr]::Zero)|Out-Null
   Wait-Until {[SmokeWindows]::GetProp($window,'RustMediaState').ToInt64() -eq 3} 'Space resume failed'
   $seek=[SmokeWindows]::Children($window)|Where-Object{[SmokeWindows]::GetDlgCtrlID($_) -eq 120}|Select-Object -First 1
   [VlcUi]::SetFocus($seek)|Out-Null
   [VlcUi]::SendMessage($seek,0x405,[IntPtr]1,[IntPtr]600)|Out-Null
   [SmokeWindows]::PostMessage($window,0x114,[IntPtr]::Zero,$seek)|Out-Null
   Wait-Until {[SmokeWindows]::GetProp($window,'RustMediaTime').ToInt64() -ge 4000} 'Progress seek failed'
   $thumb=[VlcUi]::SendMessage($seek,0x400,[IntPtr]::Zero,[IntPtr]::Zero).ToInt64()
   Wait-Until {[VlcUi]::SendMessage($seek,0x400,[IntPtr]::Zero,[IntPtr]::Zero).ToInt64() -ne $thumb} 'Progress thumb froze after focus'
   Write-Output 'SEEK_WITH_FOCUSED_PROGRESS_UPDATES_PASS'
   [SmokeWindows]::PostMessage($window,0x100,[IntPtr]27,[IntPtr]::Zero)|Out-Null
   Wait-Until {([SmokeWindows]::GetWindowLongPtr($window,-16).ToInt64() -band 0x00C00000) -ne 0} 'Escape window mode failed'
   if(![SmokeWindows]::IsWindowVisible($window)){throw 'Window mode parent was hidden'}
   Wait-Until {([SmokeWindows]::Children($window)|Where-Object{[SmokeWindows]::IsWindowVisible($_) -and [SmokeWindows]::Class($_) -eq 'Button'}).Count -ge 4} 'Window mode controls were hidden' 10
   Screenshot $window '新版窗口播放器'
   [SmokeWindows]::PostMessage($window,0x100,[IntPtr]122,[IntPtr]::Zero)|Out-Null
   Wait-Until {([SmokeWindows]::GetWindowLongPtr($window,-16).ToInt64() -band 0x00C00000) -eq 0} 'F11 fullscreen failed'
   Start-Sleep -Milliseconds 600
   $rect=[VlcUi+Rect]::new();[VlcUi]::GetWindowRect($window,[ref]$rect)|Out-Null
   [VlcUi]::SetCursorPos($rect.left+40,$rect.bottom-30)|Out-Null
   Wait-Until {([SmokeWindows]::Children($window)|Where-Object{[SmokeWindows]::IsWindowVisible($_) -and [SmokeWindows]::Class($_) -eq 'Button'}).Count -ge 4} 'Fullscreen dock reveal failed'
   Screenshot $window '新版全屏控制栏'
   [VlcUi]::SetCursorPos(10,10)|Out-Null;Start-Sleep -Seconds 4
   if([SmokeWindows]::Children($window)|Where-Object{[SmokeWindows]::IsWindowVisible($_) -and [SmokeWindows]::Class($_) -eq 'Button'}){throw 'Fullscreen dock did not hide'}
   Write-Output 'PAUSE_11_SECONDS_RESUME_ESC_F11_DOCK_SHOW_HIDE_PASS'
  }
  Close-Video $viewer $window;Start-Sleep -Seconds 2
  # Simulate a fresh shell with a different LOCALAPPDATA; installed app must
  # recover its saved physical authorization root, not inherit the build host.
  $start=[Diagnostics.ProcessStartInfo]::new();$start.FileName=$installed;$start.UseShellExecute=$false;$start.CreateNoWindow=$true
  $start.ArgumentList.Add('--autostart');$start.Environment['LOCALAPPDATA']=Join-Path $workspace 'fresh-shell-data'
  [void]$start.Environment.Remove('OFFLINE_LICENSE_VLC_DATA_ROOT')
  $viewer=[Diagnostics.Process]::Start($start);$window=Window $viewer 'RustVlcOfflineWindow' '*测试构建*'
  Wait-Until {([SmokeWindows]::GetWindowLongPtr($window,-16).ToInt64() -band 0x00C00000) -eq 0} 'Fresh-shell startup authorization resume failed'
  Write-Output ($ext.ToUpper()+'_REGISTERED_PHYSICAL_STARTUP_FRESH_ENV_AUTH_RESUME_PASS')
  Close-Video $viewer $window
  Run-Rust $tool @('--disable-startup',$content)
  if((Get-ItemProperty -LiteralPath $runKey).PSObject.Properties.Name -contains $startupName){throw 'Test startup cleanup failed'}
  $installed=$null
 }
 Write-Output 'ALL_VIDEO_E2E_PASS_NO_REAL_REBOOT_OR_LOGOFF'
}finally{
 if($viewer -and !$viewer.HasExited){Close-Video $viewer $window}
 # A failed old PID may already have been replaced by the watchdog. Close only
 # this test's exact executable path; never leave its replacement unattended.
 if($content){Get-CimInstance Win32_Process -Filter "Name='content-mp4.exe' OR Name='content-mkv.exe' OR Name='content-avi.exe' OR Name='content-mov.exe' OR Name='content-webm.exe'" | Where-Object {$_.ExecutablePath -eq $content} | ForEach-Object {[SmokeWindows]::ForProcess($_.ProcessId) | Where-Object {[SmokeWindows]::Class($_) -eq 'RustVlcOfflineWindow'} | ForEach-Object {[SmokeWindows]::PostMessage($_,0x10,[IntPtr]::Zero,[IntPtr]::Zero)|Out-Null}}}
 if($content -and (Test-Path -LiteralPath $content)){Run-Rust $tool @('--disable-startup',$content)}
 if($null -ne $savedPref){[IO.File]::WriteAllBytes($pref,$savedPref)}elseif(Test-Path -LiteralPath $pref){Remove-Item -LiteralPath $pref}
}
