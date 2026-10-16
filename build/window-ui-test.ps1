$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'ui-common.ps1')
. (Join-Path $PSScriptRoot 'run-rust.ps1')
Add-Type -AssemblyName System.Drawing
Add-Type @'
using System;using System.Runtime.InteropServices;
public static class WindowFormNative{
 [StructLayout(LayoutKind.Sequential)]public struct Rect{public int left,top,right,bottom;}
 [DllImport("user32.dll")]public static extern IntPtr SendMessage(IntPtr h,uint m,IntPtr w,IntPtr l);
 [DllImport("user32.dll")]public static extern bool ShowWindow(IntPtr h,int mode);
 [DllImport("user32.dll")]public static extern bool GetWindowRect(IntPtr h,out Rect rect);
 [DllImport("user32.dll")]public static extern bool PrintWindow(IntPtr h,IntPtr dc,uint flags);
 [DllImport("user32.dll")]public static extern bool SetProcessDPIAware();
}
'@
[WindowFormNative]::SetProcessDPIAware()|Out-Null
$tool=Join-Path $base 'target\debug\offline-license-rust.exe'
$space=Join-Path $base ('work\window-ui-'+[guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $space|Out-Null
$settings=Join-Path $env:LOCALAPPDATA 'OfflineLicenseVlcRust\issuer-settings.json'
$language=Join-Path $env:LOCALAPPDATA 'OfflineLicenseVlcRust\ui-language.txt'
$oldSettings=if(Test-Path -LiteralPath $settings){[IO.File]::ReadAllBytes($settings)}else{$null}
$oldLanguage=if(Test-Path -LiteralPath $language){[IO.File]::ReadAllBytes($language)}else{$null}
[IO.Directory]::CreateDirectory((Split-Path $settings -Parent))|Out-Null
[IO.File]::WriteAllText($settings,'{"days":30,"value":60,"unit":0}')
[IO.File]::WriteAllText($language,'zh')
function Control($window,[int]$id){$h=[SmokeWindows]::Children($window)|Where-Object{[SmokeWindows]::GetDlgCtrlID($_) -eq $id}|Select-Object -First 1;if(!$h){throw ('Missing control '+$id)};return $h}
function Enabled($h){return [Windows.Automation.AutomationElement]::FromHandle($h).Current.IsEnabled}
function Wait-Check([scriptblock]$condition,[string]$message){for($i=0;$i -lt 100;$i++){if(&$condition){return};Start-Sleep -Milliseconds 100};throw $message}
function Set-Value($h,[string]$value){[SmokeWindows]::SendMessage($h,0xC,[IntPtr]::Zero,$value)|Out-Null;Start-Sleep -Milliseconds 120}
function Set-Unit($window,[int]$unit){[WindowFormNative]::SendMessage((Control $window 23),0x14E,[IntPtr]$unit,[IntPtr]::Zero)|Out-Null;[SmokeWindows]::PostMessage($window,0x111,[IntPtr](23 -bor (1 -shl 16)),[IntPtr]::Zero)|Out-Null;Start-Sleep -Milliseconds 150}
function Close-App($process,$window){if($process -and !$process.HasExited){[SmokeWindows]::PostMessage($window,0x10,[IntPtr]::Zero,[IntPtr]::Zero)|Out-Null;if(!$process.WaitForExit(8000)){throw 'Normal close timed out'}}}
function Capture($window,[string]$filename){
 [WindowFormNative]::ShowWindow($window,9)|Out-Null;Start-Sleep -Milliseconds 500
 $rect=[WindowFormNative+Rect]::new();[WindowFormNative]::GetWindowRect($window,[ref]$rect)|Out-Null
 $image=[Drawing.Bitmap]::new($rect.right-$rect.left,$rect.bottom-$rect.top);$g=[Drawing.Graphics]::FromImage($image);$dc=$g.GetHdc()
 try{if(![WindowFormNative]::PrintWindow($window,$dc,2)){throw 'UI capture failed'}}finally{$g.ReleaseHdc($dc)}
 $image.Save((Join-Path $base ('设计参考\'+$filename+'.png')));$g.Dispose();$image.Dispose()
}
$admin=$null;$viewer=$null;$content=$null;$window=[IntPtr]::Zero;$viewerWindow=[IntPtr]::Zero
try{
 $admin=Start-Process -FilePath $tool -WindowStyle Hidden -PassThru;$window=Window $admin 'RustVlcOfflineWindow' '*测试构建*'
 if([SmokeWindows]::Text((Control $window 22)) -ne '60'){throw 'Default import window changed unexpectedly'}
 foreach($id in @(103,104,105,106,107)){if(Enabled (Control $window $id)){throw ('Prerequisite button enabled: '+$id)}}
 Set-Value (Control $window 22) '5';Set-Unit $window 1
 Wait-Check {([SmokeWindows]::Text((Control $window 24))).Contains('5 分钟')} 'Live duration summary missing'
 Close-App $admin $window
 $admin=Start-Process -FilePath $tool -WindowStyle Hidden -PassThru;$window=Window $admin 'RustVlcOfflineWindow' '*测试构建*'
 if([SmokeWindows]::Text((Control $window 22)) -ne '5' -or [WindowFormNative]::SendMessage((Control $window 23),0x147,[IntPtr]::Zero,[IntPtr]::Zero).ToInt64() -ne 1){throw 'Window value/unit were not remembered'}
 Write-Output 'DEFAULT60_UNIT_CONVERSION_LIVE_SUMMARY_RESTART_MEMORY_PASS'
 $key=Get-ChildItem -LiteralPath (Join-Path $base 'work') -Directory -Filter 'ui-language-*'|ForEach-Object{Get-ChildItem -LiteralPath $_.FullName -Filter test.issuerkey}|Sort-Object LastWriteTime -Descending|Select-Object -First 1 -ExpandProperty FullName
 if(!$key){throw 'Run ui-test.ps1 to prepare a synthetic test issuer'}
 $source=Join-Path $space 'content.txt';[IO.File]::WriteAllText($source,'custom-window-original-content')
 $content=Join-Path $space 'content.exe';$report=$content+'.json';$license=$content+'.lic'
 Run-Rust $tool @('--pack',$source,$key,$content) 'test-only-password'
 Run-Rust $tool @('--enroll',$content,$report)
 Click (Control $window 101);FileDialog $admin $key
 $prompt=Window $admin 'RustOfflinePrompt' '密钥口令'
 $edit=[SmokeWindows]::Children($prompt)|Where-Object{[SmokeWindows]::Class($_) -eq 'Edit'}|Select-Object -First 1
 Set-Value $edit 'test-only-password';Click (Button $prompt '确定')
 Wait-Check {Enabled (Control $window 106)} 'Issuer unlock did not enable package selection'
 Click (Control $window 106);FileDialog $admin $content
 Wait-Check {Enabled (Control $window 107)} 'Selected package did not enable issuance'
 if(Enabled (Control $window 105)){throw 'Existing package should not enable repackaging stale preview'}
 foreach($value in @('0','','-1','1.5','9999999999999999999999999999')){
  Set-Value (Control $window 22) $value
  Wait-Check {!(Enabled (Control $window 107))} ('Invalid value allowed issuance: '+$value)
  if(![SmokeWindows]::Text((Control $window 24))){throw 'Invalid input has no explanation'}
 }
 Set-Value (Control $window 22) '5';Set-Unit $window 1
 Wait-Check {Enabled (Control $window 107)} 'Corrected value did not re-enable issuance'
 Write-Output 'ISSUER_PACKAGE_GATING_INVALID_INPUT_AND_RECOVERY_PASS'
 Capture $window '2.4.0-授权工作台-中文'
 [SmokeWindows]::PostMessage($window,0x111,[IntPtr]202,[IntPtr]::Zero)|Out-Null
 Wait-Check {([SmokeWindows]::Text((Control $window 24))).Contains('5 minutes')} 'English form summary missing'
 if([SmokeWindows]::Text((Control $window 22)) -ne '5'){throw 'Language switch altered input'}
 Capture $window '2.4.0-授权工作台-English'
 [SmokeWindows]::PostMessage($window,0x111,[IntPtr]201,[IntPtr]::Zero)|Out-Null
 Wait-Check {([SmokeWindows]::Text((Control $window 24))).Contains('5 分钟')} 'Chinese summary restore missing'
 Write-Output 'BILINGUAL_LABEL_SUMMARY_INPUT_PRESERVATION_PASS'
 Click (Control $window 107);FileDialog $admin $report;FileDialog $admin $license
 $confirmation=Window $admin '#32770' '立即签发'
 $body=([SmokeWindows]::Children($confirmation)|ForEach-Object{[SmokeWindows]::Text($_)}) -join "`n"
 if(!$body.Contains('5 分钟') -or !$body.Contains('30 天') -or $body.Contains('60 秒')){throw 'Signing confirmation has stale or wrong settings'}
 $cancel=[SmokeWindows]::Children($confirmation)|Where-Object{[SmokeWindows]::GetDlgCtrlID($_) -eq 2}|Select-Object -First 1;Click $cancel
 Wait-Check {![SmokeWindows]::IsWindowVisible($confirmation)} 'Cancel did not close confirmation'
 if(Test-Path -LiteralPath $license){throw 'Cancel created a license'}
 Write-Output 'CONFIGURED_CONFIRMATION_CANCEL_NO_LICENSE_PASS'
 Click (Control $window 107);FileDialog $admin $report;FileDialog $admin $license
 $confirmation=Window $admin '#32770' '立即签发'
 $ok=[SmokeWindows]::Children($confirmation)|Where-Object{[SmokeWindows]::GetDlgCtrlID($_) -eq 1}|Select-Object -First 1;Click $ok
 Wait-Check {Test-Path -LiteralPath $license} 'GUI did not sign custom-window license'
 $signed=Get-Content -Raw -LiteralPath $license|ConvertFrom-Json
 $body=[Text.Encoding]::UTF8.GetString([Convert]::FromBase64String($signed.BodyBase64))|ConvertFrom-Json
 if(($body.ActivateBeforeUtcSeconds-$body.IssuedAtUtcSeconds) -ne 300 -or $body.Days -ne 30){throw 'Signed license does not match GUI settings'}
 Write-Output 'GUI_SIGNED_5_MINUTE_WINDOW_30_DAY_LICENSE_PASS'
 # Also verify the optional CLI value while preserving the historical default.
 Run-Rust $tool @('--issue',$key,($content+'.olmeta'),$report,'30',($license+'.cli'),'7200') 'test-only-password'
 $cli=Get-Content -Raw -LiteralPath ($license+'.cli')|ConvertFrom-Json
 $cliBody=[Text.Encoding]::UTF8.GetString([Convert]::FromBase64String($cli.BodyBase64))|ConvertFrom-Json
 if(($cliBody.ActivateBeforeUtcSeconds-$cliBody.IssuedAtUtcSeconds) -ne 7200){throw 'CLI custom window mismatch'}
 Write-Output 'CLI_OPTIONAL_TWO_HOUR_WINDOW_PASS'
 $viewer=Start-Process -FilePath $content -WindowStyle Hidden -PassThru;$viewerWindow=Window $viewer 'RustVlcOfflineWindow' '*测试构建*'
 Click (Button $viewerWindow '导入固态链密钥文件');FileDialog $viewer $license
 Wait-Check {([SmokeWindows]::Children($viewerWindow)|Where-Object{[SmokeWindows]::Class($_) -like '*RICHEDIT*' -and [SmokeWindows]::Text($_) -eq 'custom-window-original-content'}).Count -gt 0} 'Recipient rejected custom-window signed license'
 Write-Output 'RECIPIENT_ACCEPTS_NON60_SIGNED_WINDOW_ORIGINAL_CONTENT_PASS'
 Close-App $viewer $viewerWindow
 Run-Rust $tool @('--disable-startup',$content)
 Write-Output 'CUSTOM_WINDOW_GUI_E2E_PASS_NO_REAL_REBOOT'
}finally{
 if($viewer -and !$viewer.HasExited){Close-App $viewer $viewerWindow}
 if($admin -and !$admin.HasExited){Close-App $admin $window}
 if($content -and (Test-Path -LiteralPath $content)){Run-Rust $tool @('--disable-startup',$content)}
 if($null -ne $oldSettings){[IO.File]::WriteAllBytes($settings,$oldSettings)}elseif(Test-Path -LiteralPath $settings){Remove-Item -LiteralPath $settings}
 if($null -ne $oldLanguage){[IO.File]::WriteAllBytes($language,$oldLanguage)}elseif(Test-Path -LiteralPath $language){Remove-Item -LiteralPath $language}
}
