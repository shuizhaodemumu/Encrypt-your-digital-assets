param([string]$Version='2.4.1',[switch]$CheckSideArtwork,[switch]$FrameFree)
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'ui-common.ps1')
Add-Type -AssemblyName System.Drawing
Add-Type @'
using System; using System.Runtime.InteropServices;
public static class RepaintNative {
 [StructLayout(LayoutKind.Sequential)]public struct Rect{public int left,top,right,bottom;}
 [DllImport("user32.dll")]public static extern bool RedrawWindow(IntPtr h,IntPtr r,IntPtr region,uint flags);
 [DllImport("user32.dll")]public static extern bool PrintWindow(IntPtr h,IntPtr dc,uint flags);
 [DllImport("user32.dll")]public static extern bool GetClientRect(IntPtr h,out Rect r);
 [DllImport("user32.dll")]public static extern bool ShowWindow(IntPtr h,int m);
 [DllImport("user32.dll")]public static extern bool MoveWindow(IntPtr h,int x,int y,int w,int height,bool redraw);
 [DllImport("user32.dll")]public static extern uint GetGuiResources(IntPtr p,uint flags);
 [DllImport("user32.dll")]public static extern bool SetProcessDPIAware();
}
'@
[RepaintNative]::SetProcessDPIAware()|Out-Null
function Capture-Check($h,$name){
 [RepaintNative]::RedrawWindow($h,[IntPtr]::Zero,[IntPtr]::Zero,0x185)|Out-Null
 $r=[RepaintNative+Rect]::new();[RepaintNative]::GetClientRect($h,[ref]$r)|Out-Null
 $bmp=[Drawing.Bitmap]::new($r.right,$r.bottom);$g=[Drawing.Graphics]::FromImage($bmp);$dc=$g.GetHdc()
 try{if(![RepaintNative]::PrintWindow($h,$dc,3)){throw 'Capture failed'}}finally{$g.ReleaseHdc($dc);$g.Dispose()}
 try{
  $dark=0;$count=0;$colors=[Collections.Generic.HashSet[int]]::new()
  foreach($y in @(15,75,380,400,620)){foreach($x in 5..($r.right-6)){if(($x%9)-ne 0){continue};$c=$bmp.GetPixel($x,$y);if($c.R -lt 40 -and $c.G -lt 40 -and $c.B -lt 40){$dark++};$count++;[void]$colors.Add($c.ToArgb())}}
  if($dark/$count -gt 0.01){throw "Black background detected: $dark/$count"}
  if($colors.Count -lt 10){throw 'Full-window background lacks artwork variation'}
  if($CheckSideArtwork){
   $ink=0
   for($y=420;$y -lt [Math]::Min(650,$r.bottom-100);$y+=4){for($x=$r.right-350;$x -lt $r.right-40;$x+=4){$c=$bmp.GetPixel($x,$y);if(($c.R+$c.G+$c.B)/3 -lt 190){$ink++}}}
   if($ink -lt 100){throw 'Right-side illustration missing or washed out'}
   Write-Output "ILLUSTRATION_VISIBLE_PIXELS_PASS: $ink"
  }
  if($FrameFree){
   foreach($x in @(($r.right-380),($r.right-28))){
    foreach($y in @(470,500,530)){
     $c=$bmp.GetPixel($x,$y)
     if($c.R -eq 255 -and $c.G -eq 255 -and $c.B -eq 255){throw 'White illustration card edge remains'}
    }
   }
   Write-Output 'NO_WHITE_CARD_EDGE_PIXELS_PASS: 6/6'
  }
  $bmp.Save((Join-Path $base ('设计参考\'+$name.Replace('2.4.1-',"$Version-")+'.png')))
 }finally{$bmp.Dispose()}
}
$pref=Join-Path $env:LOCALAPPDATA 'OfflineLicenseVlcRust\ui-language.txt'
$old=if(Test-Path -LiteralPath $pref){[IO.File]::ReadAllBytes($pref)}else{$null}
$p=$null;$h=[IntPtr]::Zero
try{
 $p=Start-Process -FilePath (Join-Path $base 'target\debug\offline-license-rust.exe') -WindowStyle Hidden -PassThru
 $h=Window $p 'RustVlcOfflineWindow' '*'
 if([SmokeWindows]::Text($h) -notmatch 'TEST BUILD|测试构建'){throw 'Requires watchdog-test-mode'}
 [RepaintNative]::ShowWindow($h,9)|Out-Null
 [SmokeWindows]::PostMessage($h,0x111,[IntPtr]201,[IntPtr]::Zero)|Out-Null;Start-Sleep -Milliseconds 300
 Capture-Check $h '2.4.1-全窗口淡背景-中文'
 Click (Button $h '创建发行者密钥');$dialog=Window $p '#32770' '*'
 # Force redraw while WM_COMMAND owns App and its modal message loop runs.
 Capture-Check $h '2.4.1-弹窗期间重绘'
 [SmokeWindows]::PostMessage($dialog,0x111,[IntPtr]2,[IntPtr]::Zero)|Out-Null;Start-Sleep -Milliseconds 200
 'MODAL_NESTED_PAINT_OWNERDRAW_NO_BLACK_PASS'
 $baseline=[RepaintNative]::GetGuiResources($p.Handle,0)
 for($i=0;$i -lt 60;$i++){
  [SmokeWindows]::PostMessage($h,0x111,[IntPtr](201+($i%2)),[IntPtr]::Zero)|Out-Null
  [RepaintNative]::MoveWindow($h,40,40,(1180+($i%2)*100),(880+($i%2)*40),$true)|Out-Null
  [RepaintNative]::RedrawWindow($h,[IntPtr]::Zero,[IntPtr]::Zero,0x185)|Out-Null
  Start-Sleep -Milliseconds 20
 }
 Start-Sleep -Milliseconds 300
 $after=[RepaintNative]::GetGuiResources($p.Handle,0)
 if($after-$baseline -gt 8){throw "GDI resource growth: $baseline->$after"}
 Capture-Check $h '2.4.1-全窗口淡背景-English'
 "RESIZE_LANGUAGE_60_CYCLES_GDI_STABLE_PASS: $baseline->$after"
 [RepaintNative]::ShowWindow($h,6)|Out-Null;[RepaintNative]::ShowWindow($h,9)|Out-Null
 Capture-Check $h '2.4.1-最小化恢复重绘'
 'RESTORE_RESIZE_FULL_CANVAS_READABILITY_PASS'
}finally{
 if($p -and !$p.HasExited){[SmokeWindows]::PostMessage($h,0x10,[IntPtr]::Zero,[IntPtr]::Zero)|Out-Null;if(!$p.WaitForExit(5000)){throw 'Close failed'}}
 if($null -ne $old){[IO.File]::WriteAllBytes($pref,$old)}elseif(Test-Path -LiteralPath $pref){Remove-Item -LiteralPath $pref}
}
