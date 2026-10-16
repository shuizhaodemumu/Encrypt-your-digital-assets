$ErrorActionPreference='Stop'
$base=Split-Path $PSScriptRoot -Parent
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes
Add-Type -Path (Join-Path $PSScriptRoot 'native-ui.cs')
function Click($h){[SmokeWindows]::PostMessage($h,0xF5,[IntPtr]::Zero,[IntPtr]::Zero)|Out-Null}
function Window($process,$class,$title){for($i=0;$i -lt 100;$i++){foreach($h in [SmokeWindows]::ForProcess($process.Id)){if([SmokeWindows]::Class($h) -like $class -and [SmokeWindows]::Text($h) -like $title){return $h}};Start-Sleep -Milliseconds 100};throw "Missing $class $title"}
function Button($window,$label){$h=[SmokeWindows]::Children($window)|Where-Object{[SmokeWindows]::Text($_) -eq $label}|Select-Object -First 1;if(!$h){throw "Missing button $label"};return $h}
function FileDialog($process,$path){
 Write-Host "Selecting $path"
 $deadline=[DateTime]::UtcNow.AddSeconds(20)
 while([DateTime]::UtcNow -lt $deadline){
  foreach($d in [SmokeWindows]::ForProcess($process.Id)){
   if([SmokeWindows]::Class($d) -ne '#32770' -or ![SmokeWindows]::IsWindowVisible($d)){continue}
   $children=[SmokeWindows]::Children($d)
   $edit=$children|Where-Object{[SmokeWindows]::Class($_) -eq 'Edit' -and [SmokeWindows]::GetDlgCtrlID($_) -eq 1148}|Select-Object -First 1
   if(!$edit){$edit=$children|Where-Object{[SmokeWindows]::Class($_) -eq 'Edit' -and [SmokeWindows]::GetDlgCtrlID($_) -eq 1001 -and [SmokeWindows]::Class([SmokeWindows]::GetParent($_)) -eq 'ComboBox'}|Select-Object -First 1}
   $ok=$children|Where-Object{[SmokeWindows]::GetDlgCtrlID($_) -eq 1 -and [SmokeWindows]::Class($_) -eq 'Button'}|Select-Object -First 1
   $uiaEdit=$null;$uiaOk=$null
   if(!$edit -or !$ok){try{
    $a=[Windows.Automation.AutomationElement]::FromHandle($d)
    $all=$a.FindAll([Windows.Automation.TreeScope]::Descendants,[Windows.Automation.Condition]::TrueCondition)
    $uiaEdit=$all|Where-Object{$_.Current.ControlType -eq [Windows.Automation.ControlType]::Edit -and ($_.Current.AutomationId -in @('1001','1148') -or $_.Current.Name -like '*文件名*' -or $_.Current.Name -like '*File name*')}|Select-Object -Last 1
    $uiaOk=$all|Where-Object{$_.Current.ControlType -eq [Windows.Automation.ControlType]::Button -and $_.Current.AutomationId -eq '1'}|Select-Object -First 1
   }catch{continue}}
   if((!$edit -and !$uiaEdit) -or (!$ok -and !$uiaOk)){continue}
   try{
    if($edit){[SmokeWindows]::SendMessage($edit,0xC,[IntPtr]::Zero,$path)|Out-Null;if([SmokeWindows]::Text($edit) -ne $path){continue}}
    else{$uiaEdit.GetCurrentPattern([Windows.Automation.ValuePattern]::Pattern).SetValue($path)}
    if($ok){Click $ok}else{$uiaOk.GetCurrentPattern([Windows.Automation.InvokePattern]::Pattern).Invoke()}
    for($i=0;$i -lt 100;$i++){
     if(![SmokeWindows]::IsWindowVisible($d)){Start-Sleep -Milliseconds 200;return}
     Start-Sleep -Milliseconds 50
    }
    throw 'Filename selected but file dialog did not complete'
   }catch{Start-Sleep -Milliseconds 100}
  }
  Start-Sleep -Milliseconds 100
 }
 foreach($h in [SmokeWindows]::ForProcess($process.Id)){Write-Host ('DIALOG_DIAGNOSTIC '+[SmokeWindows]::Class($h)+' / '+[SmokeWindows]::Text($h))}
 throw 'File dialog filename control/completion not found'
}
