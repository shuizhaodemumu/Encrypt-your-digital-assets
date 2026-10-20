$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'ui-common.ps1')
Add-Type @'
using System;using System.Runtime.InteropServices;
public static class IdentityNative{
 [StructLayout(LayoutKind.Sequential)]public struct R{public int left,top,right,bottom;}
 [StructLayout(LayoutKind.Sequential)]public struct P{public int x,y;}
 [DllImport("user32.dll")]public static extern bool GetWindowRect(IntPtr h,out R r);
 [DllImport("user32.dll")]public static extern bool GetClientRect(IntPtr h,out R r);
 [DllImport("user32.dll")]public static extern bool ClientToScreen(IntPtr h,ref P p);
 [DllImport("user32.dll")]public static extern bool MoveWindow(IntPtr h,int x,int y,int w,int height,bool redraw);
 [DllImport("user32.dll")]public static extern IntPtr SendMessage(IntPtr h,uint m,IntPtr w,IntPtr l);
 [DllImport("user32.dll")]public static extern bool SetProcessDPIAware();
}
'@
[IdentityNative]::SetProcessDPIAware()|Out-Null
$pref=Join-Path $env:LOCALAPPDATA 'OfflineLicenseVlcRust\ui-language.txt'
$old=if(Test-Path -LiteralPath $pref){[IO.File]::ReadAllBytes($pref)}else{$null}
$p=$null
try{
 [IO.File]::WriteAllText($pref,'zh')
 $p=Start-Process -FilePath (Join-Path $base 'target\debug\offline-license-rust.exe') -WindowStyle Hidden -PassThru
 $h=Window $p 'RustVlcOfflineWindow' '*'
 $icon=[IdentityNative]::SendMessage($h,0x7F,[IntPtr]0,[IntPtr]::Zero)
 if($icon -eq [IntPtr]::Zero){throw 'Window has no small application icon'}
 Click (Button $h '中文 / English')
 Start-Sleep -Milliseconds 250
 Button $h 'Create issuer key'|Out-Null
 Click (Button $h '中文 / English')
 Start-Sleep -Milliseconds 250
 Button $h '创建发行者密钥'|Out-Null
 foreach($size in @(@(1120,760),@(1180,880),@(1380,980))){
  [IdentityNative]::MoveWindow($h,20,20,$size[0],$size[1],$true)|Out-Null
  Start-Sleep -Milliseconds 180
  $client=[IdentityNative+R]::new();[IdentityNative]::GetClientRect($h,[ref]$client)|Out-Null
  $origin=[IdentityNative+P]::new();[IdentityNative]::ClientToScreen($h,[ref]$origin)|Out-Null
  $rects=@()
  foreach($child in [SmokeWindows]::Children($h)){
   $id=[SmokeWindows]::GetDlgCtrlID($child)
   if($id -notin @(102,101,103,104,105,106,107,108,203,20,22,23)){continue}
   $r=[IdentityNative+R]::new();[IdentityNative]::GetWindowRect($child,[ref]$r)|Out-Null
   if($r.left -lt $origin.x -or $r.right -gt $origin.x+$client.right -or $r.top -lt $origin.y -or $r.bottom -gt $origin.y+$client.bottom-60){throw "Control $id clipped at $size"}
   foreach($previous in $rects){
    if($r.left -lt $previous.r.right -and $r.right -gt $previous.r.left -and $r.top -lt $previous.r.bottom -and $r.bottom -gt $previous.r.top){throw "Controls $id and $($previous.id) overlap"}
   }
   $rects+=@{id=$id;r=$r}
  }
  if($rects.Count -ne 12){throw 'Missing workspace controls'}
 }
 'WINDOW_ICON_LANGUAGE_BUTTON_MINIMUM_AND_LARGE_LAYOUT_PASS'
}finally{
 if($p -and !$p.HasExited){[SmokeWindows]::PostMessage($h,0x10,[IntPtr]::Zero,[IntPtr]::Zero)|Out-Null;if(!$p.WaitForExit(5000)){throw 'Close failed'}}
 if($null -ne $old){[IO.File]::WriteAllBytes($pref,$old)}elseif(Test-Path -LiteralPath $pref){Remove-Item -LiteralPath $pref}
}
