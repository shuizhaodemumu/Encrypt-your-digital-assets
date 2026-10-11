param([string]$Source,[string]$Output)
$ErrorActionPreference='Stop'
$taskUpdater=Join-Path $PSScriptRoot 'ViewerUpdater-OfficeCompat.exe'
if(-not(Test-Path -LiteralPath $taskUpdater)){$taskUpdater=Join-Path (Split-Path $PSScriptRoot -Parent) '程序\ViewerUpdater-OfficeCompat.exe'}
$taskInteractive=(-not $Source -or -not $Output)
try{
    if(-not(Test-Path -LiteralPath $taskUpdater)){throw 'Missing ViewerUpdater-OfficeCompat.exe'}
    if($taskInteractive){
        Add-Type -AssemblyName System.Windows.Forms
        $taskPick=[Windows.Forms.OpenFileDialog]::new()
        $taskPick.Title='选择原 Rust Office 内容 EXE / Select original Rust Office content EXE'
        $taskPick.Filter='Content EXE (*.exe)|*.exe'
        if($taskPick.ShowDialog() -ne [Windows.Forms.DialogResult]::OK){exit 0}
        $Source=$taskPick.FileName
        $taskSave=[Windows.Forms.SaveFileDialog]::new()
        $taskSave.Title='保存升级后的新 EXE / Save upgraded EXE'
        $taskSave.Filter='Content EXE (*.exe)|*.exe'
        $taskSave.FileName=[IO.Path]::GetFileNameWithoutExtension($Source)+'-loop-fixed.exe'
        if($taskSave.ShowDialog() -ne [Windows.Forms.DialogResult]::OK){exit 0}
        $Output=$taskSave.FileName
    }
    $Source=[IO.Path]::GetFullPath($Source);$Output=[IO.Path]::GetFullPath($Output)
    if($Source -eq $Output){throw '请保存为新文件，保留原文件 / Save to a new file and keep the original'}
    $taskInfo=[Diagnostics.ProcessStartInfo]::new()
    $taskInfo.FileName=$taskUpdater;$taskInfo.UseShellExecute=$false;$taskInfo.CreateNoWindow=$true
    $taskInfo.RedirectStandardError=$true
    # Quoted fixed argv, including paths with spaces and Chinese characters.
    $taskArgs=@('--upgrade',$Source,$Output)
    foreach($taskArg in $taskArgs){if($taskArg.Contains('"') -or $taskArg.EndsWith('\')){throw 'Invalid file path'}}
    $taskInfo.Arguments=($taskArgs|ForEach-Object{'"'+$_+'"'}) -join ' '
    $taskProcess=[Diagnostics.Process]::Start($taskInfo)
    $taskError=$taskProcess.StandardError.ReadToEnd();$taskProcess.WaitForExit()
    if($taskProcess.ExitCode){throw $taskError}
    if($taskInteractive){[Windows.Forms.MessageBox]::Show('已生成新 EXE，原密文已校验保持一致。关闭旧程序后使用新文件；适用于原 Rust 修复 Office 版。 / New EXE created; encrypted content verified unchanged. Close the old application before opening it. Intended for the original Rust Office edition.','升级完成 / Upgrade complete')|Out-Null}
    else{Write-Output 'UPGRADE_OK: encrypted content preserved'}
}catch{
    if($taskInteractive){[Windows.Forms.MessageBox]::Show($_.Exception.Message,'升级失败 / Upgrade failed')|Out-Null}else{throw}
    exit 1
}
